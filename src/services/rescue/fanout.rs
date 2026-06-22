use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    domain::{AccountType, AnimalType, Author, Post, PostType, RescueOperationalSummary},
    services::{
        geo::haversine_km,
        notifications::{
            push_platform_from_str, AlertRecipient, NotificationAction, PushPlatform, RescueAlert,
        },
    },
};

const WORKER_INTERVAL_SECONDS: u64 = 15;
const CLAIM_BATCH_SIZE: i64 = 20;
const ACTIVE_SUBSCRIPTION_MAX_AGE_MINUTES: i64 = 24 * 60;
const MAX_CANDIDATES_PER_ATTEMPT: usize = 250;
const MAX_SPECIALIST_CANDIDATES_PER_ATTEMPT: usize = 80;
const MAX_RECENT_RESCUE_ALERTS_30M: i32 = 3;
const MAX_RECENT_RESCUE_ALERTS_60M: i32 = 6;
const EARTH_KM_PER_DEGREE: f64 = 111.0;
const SOFT_FANOUT_AFTER_MINUTES: i64 = 60;
const MAX_FANOUT_LIFETIME_HOURS: i64 = 6;
const SOFT_FANOUT_INTERVAL_SECONDS: i64 = 3600;
const SOFT_FANOUT_PHASE: i32 = 10;

#[derive(Clone, Copy, Debug)]
struct FanoutPhase {
    phase: i32,
    radius_km: f64,
    next_delay_seconds: i64,
    verified_escalation: bool,
}

const FANOUT_PHASES: [FanoutPhase; 5] = [
    FanoutPhase {
        phase: 1,
        radius_km: 0.3,
        next_delay_seconds: 90,
        verified_escalation: false,
    },
    FanoutPhase {
        phase: 2,
        radius_km: 0.7,
        next_delay_seconds: 120,
        verified_escalation: false,
    },
    FanoutPhase {
        phase: 3,
        radius_km: 1.0,
        next_delay_seconds: 180,
        verified_escalation: false,
    },
    FanoutPhase {
        phase: 4,
        radius_km: 3.0,
        next_delay_seconds: 300,
        verified_escalation: false,
    },
    FanoutPhase {
        phase: 5,
        radius_km: 25.0,
        next_delay_seconds: 300,
        verified_escalation: true,
    },
];

#[derive(Clone, Copy, Debug)]
struct EscalationPhase {
    phase: i32,
    radius_km: f64,
    next_delay_seconds: i64,
    strategy: &'static str,
}

const ESCALATION_PHASES: [EscalationPhase; 4] = [
    EscalationPhase {
        phase: 6,
        radius_km: 10.0,
        next_delay_seconds: 300,
        strategy: "specialist_local",
    },
    EscalationPhase {
        phase: 7,
        radius_km: 30.0,
        next_delay_seconds: 600,
        strategy: "specialist_regional",
    },
    EscalationPhase {
        phase: 8,
        radius_km: 100.0,
        next_delay_seconds: 900,
        strategy: "specialist_state",
    },
    EscalationPhase {
        phase: 9,
        radius_km: 300.0,
        next_delay_seconds: 1800,
        strategy: "environmental_agency",
    },
];

const SOFT_FANOUT: FanoutPhase = FanoutPhase {
    phase: SOFT_FANOUT_PHASE,
    radius_km: 10.0,
    next_delay_seconds: SOFT_FANOUT_INTERVAL_SECONDS,
    verified_escalation: false,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueResponseRecord {
    pub id: String,
    pub post_id: String,
    pub rescue_session_id: Option<String>,
    pub user_id: String,
    pub action: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
struct FanoutState {
    id: Uuid,
    post_id: Uuid,
    current_phase: i32,
    created_at: DateTime<Utc>,
}

#[derive(Debug)]
struct Candidate {
    user_id: Uuid,
    push_token: String,
    platform: PushPlatform,
    distance_km: f64,
    score: f64,
}

#[derive(Debug)]
struct SpecialistCandidate {
    provider_id: Option<Uuid>,
    user_id: Option<Uuid>,
    name: String,
    provider_type: String,
    push_token: Option<String>,
    platform: Option<PushPlatform>,
    distance_km: f64,
    score: f64,
    contactable: bool,
}

pub fn spawn(enabled: bool, db: PgPool) {
    if !enabled {
        return;
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(WORKER_INTERVAL_SECONDS));
        loop {
            interval.tick().await;
            if let Err(error) = process_due_fanouts(&db).await {
                tracing::warn!(?error, "rescue fanout worker batch failed");
            }
        }
    });
}

pub async fn create_fanout_state_for_post(
    db: &PgPool,
    post_id: Uuid,
    rescue_session_id: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    let phase = FANOUT_PHASES[0];
    let id = Uuid::now_v7();
    let state_id = sqlx::query_scalar(
        r#"
        INSERT INTO rescue_fanout_states (
          id, post_id, rescue_session_id, current_phase, status, last_radius_km, next_run_at
        )
        VALUES ($1, $2, $3, 1, 'active', $4, now())
        ON CONFLICT (post_id)
        DO UPDATE SET
          rescue_session_id = COALESCE(rescue_fanout_states.rescue_session_id, EXCLUDED.rescue_session_id),
          status = CASE
            WHEN rescue_fanout_states.status IN ('completed', 'cancelled', 'exhausted') THEN rescue_fanout_states.status
            ELSE 'active'
          END,
          updated_at = now()
        RETURNING id
        "#,
    )
    .bind(id)
    .bind(post_id)
    .bind(rescue_session_id)
    .bind(phase.radius_km)
    .fetch_one(db)
    .await?;
    Ok(state_id)
}

pub async fn upsert_rescue_response(
    db: &PgPool,
    post_id: Uuid,
    rescue_session_id: Option<Uuid>,
    user_id: Uuid,
    action: &str,
    status: &str,
    lat: Option<f64>,
    lng: Option<f64>,
    eta_seconds: Option<i32>,
) -> Result<RescueResponseRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO rescue_responses (
          id, rescue_session_id, post_id, user_id, action, status, lat, lng, eta_seconds
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (post_id, user_id, action)
        DO UPDATE SET
          rescue_session_id = COALESCE(EXCLUDED.rescue_session_id, rescue_responses.rescue_session_id),
          status = EXCLUDED.status,
          lat = EXCLUDED.lat,
          lng = EXCLUDED.lng,
          eta_seconds = EXCLUDED.eta_seconds,
          updated_at = now()
        RETURNING id, rescue_session_id, post_id, user_id, action, status, created_at, updated_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(rescue_session_id)
    .bind(post_id)
    .bind(user_id)
    .bind(action)
    .bind(status)
    .bind(lat)
    .bind(lng)
    .bind(eta_seconds)
    .fetch_one(db)
    .await?;

    refresh_fanout_response_counts(db, post_id).await?;
    refresh_volunteer_profile_counts(db, user_id).await?;
    pause_fanout_if_confirmed(db, post_id).await?;

    Ok(RescueResponseRecord {
        id: row.get::<Uuid, _>("id").to_string(),
        rescue_session_id: row
            .get::<Option<Uuid>, _>("rescue_session_id")
            .map(|value| value.to_string()),
        post_id: row.get::<Uuid, _>("post_id").to_string(),
        user_id: row.get::<Uuid, _>("user_id").to_string(),
        action: row.get("action"),
        status: row.get("status"),
        created_at: row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
        updated_at: row.get::<DateTime<Utc>, _>("updated_at").to_rfc3339(),
    })
}

async fn refresh_volunteer_profile_counts(db: &PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE volunteer_profiles
        SET responses_count = (
              SELECT count(DISTINCT post_id)::bigint
              FROM rescue_responses
              WHERE user_id = $1
                AND status IN ('confirmed', 'arrived')
            ),
            arrived_count = (
              SELECT count(DISTINCT post_id)::bigint
              FROM rescue_responses
              WHERE user_id = $1
                AND status = 'arrived'
            ),
            updated_at = now()
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn operational_summary(
    db: &PgPool,
    post_id: Uuid,
) -> Result<RescueOperationalSummary, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
          fs.current_phase,
          COALESCE(fs.confirmed_count, 0) AS confirmed_count,
          COALESCE(fs.arrived_count, 0) AS arrived_count
        FROM rescue_fanout_states fs
        WHERE fs.post_id = $1
        "#,
    )
    .bind(post_id)
    .fetch_optional(db)
    .await?;

    let (phase, going, arrived) = row
        .map(|row| {
            (
                Some(row.get::<i32, _>("current_phase")),
                row.get::<i32, _>("confirmed_count"),
                row.get::<i32, _>("arrived_count"),
            )
        })
        .unwrap_or((None, 0, 0));

    let fanout_phase = phase.unwrap_or_default();
    let operational_label = if arrived > 0 {
        "Ajuda no local".to_string()
    } else if going == 1 {
        "1 pessoa a caminho".to_string()
    } else if going > 1 {
        format!("{going} pessoas a caminho")
    } else if fanout_phase >= 9 {
        "Acionando apoio ambiental".to_string()
    } else if fanout_phase >= 6 {
        "Buscando apoio regional".to_string()
    } else if phase.is_some() {
        "Precisa de ajuda".to_string()
    } else {
        "Resgate em coordenação".to_string()
    };

    Ok(RescueOperationalSummary {
        fanout_phase: phase,
        help_going_count: going,
        help_arrived_count: arrived,
        operational_label,
    })
}

async fn process_due_fanouts(db: &PgPool) -> Result<(), sqlx::Error> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM rescue_fanout_states
        WHERE status = 'active'
          AND next_run_at <= now()
        ORDER BY next_run_at ASC
        LIMIT $1
        "#,
    )
    .bind(CLAIM_BATCH_SIZE)
    .fetch_all(db)
    .await?;

    for id in ids {
        if let Err(error) = process_one_fanout(db, id).await {
            tracing::warn!(?error, %id, "rescue fanout attempt failed");
        }
    }

    Ok(())
}

async fn process_one_fanout(db: &PgPool, state_id: Uuid) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    let row = sqlx::query(
        r#"
        SELECT id, post_id, rescue_session_id, current_phase, created_at
        FROM rescue_fanout_states
        WHERE id = $1
          AND status = 'active'
          AND next_run_at <= now()
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .bind(state_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        tx.commit().await?;
        return Ok(());
    };

    let state = FanoutState {
        id: row.get("id"),
        post_id: row.get("post_id"),
        current_phase: row.get("current_phase"),
        created_at: row.get("created_at"),
    };

    let post_status: Option<String> =
        sqlx::query_scalar("SELECT rescue_status FROM posts WHERE id = $1")
            .bind(state.post_id)
            .fetch_optional(&mut *tx)
            .await?;

    match post_status.as_deref() {
        Some("resolved") => {
            complete_state(&mut tx, state.id, "completed").await?;
            tx.commit().await?;
            return Ok(());
        }
        Some("cancelled") | None => {
            complete_state(&mut tx, state.id, "cancelled").await?;
            tx.commit().await?;
            return Ok(());
        }
        _ => {}
    }

    let (confirmed_count, arrived_count) = count_responses(&mut tx, state.post_id).await?;
    update_state_counts(&mut tx, state.id, confirmed_count, arrived_count).await?;
    if confirmed_count > 0 {
        sqlx::query(
            r#"
            UPDATE rescue_fanout_states
            SET status = 'paused',
                confirmed_count = $2,
                arrived_count = $3,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(state.id)
        .bind(confirmed_count)
        .bind(arrived_count)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(());
    }

    let post = load_post_for_fanout(&mut tx, state.post_id).await?;
    if let Some(phase) = phase_for(state.current_phase) {
        process_standard_phase(&mut tx, &state, &post, phase, confirmed_count).await?;
        tx.commit().await?;
        return Ok(());
    }

    if let Some(phase) = escalation_phase_for(state.current_phase) {
        process_escalation_phase(&mut tx, &state, &post, phase).await?;
        tx.commit().await?;
        return Ok(());
    }

    if state.current_phase == SOFT_FANOUT_PHASE {
        process_soft_fanout_phase(&mut tx, &state, &post).await?;
        tx.commit().await?;
        return Ok(());
    }

    complete_state(&mut tx, state.id, "exhausted").await?;
    tx.commit().await
}

async fn process_standard_phase(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &FanoutState,
    post: &Post,
    phase: FanoutPhase,
    confirmed_count: i32,
) -> Result<(), sqlx::Error> {
    let candidates = ranked_candidates(tx, post, phase).await?;
    let candidate_count = candidates.len() as i32;
    let alert = alert_for_candidates(&post, phase, candidates)?;
    let recipient_count = alert.recipients.len() as i32;
    persist_rescue_alert_tx(tx, &alert).await?;

    sqlx::query(
        r#"
        INSERT INTO rescue_fanout_attempts (
          id, fanout_state_id, post_id, phase, radius_km, candidate_count,
          push_jobs_created, confirmed_count_at_run, reason
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(state.id)
    .bind(state.post_id)
    .bind(phase.phase)
    .bind(phase.radius_km)
    .bind(candidate_count)
    .bind(recipient_count)
    .bind(confirmed_count)
    .bind(if phase.verified_escalation {
        "verified_escalation"
    } else {
        "progressive_radius"
    })
    .execute(&mut **tx)
    .await?;

    let next_phase = phase.phase + 1;
    if next_phase > 5 {
        sqlx::query(
            r#"
            UPDATE rescue_fanout_states
            SET current_phase = 6,
                last_radius_km = $2,
                next_run_at = now() + ($3::text || ' seconds')::interval,
                attempts = attempts + 1,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(state.id)
        .bind(phase.radius_km)
        .bind(phase.next_delay_seconds)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            r#"
            UPDATE rescue_fanout_states
            SET current_phase = $2,
                last_radius_km = $3,
                next_run_at = now() + ($4::text || ' seconds')::interval,
                attempts = attempts + 1,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(state.id)
        .bind(next_phase)
        .bind(phase.radius_km)
        .bind(phase.next_delay_seconds)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn process_escalation_phase(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &FanoutState,
    post: &Post,
    phase: EscalationPhase,
) -> Result<(), sqlx::Error> {
    let scopes = animal_scopes_for_post(post);
    let specialists = ranked_specialist_candidates(tx, post, phase, &scopes).await?;
    let candidate_count = specialists.len() as i32;
    let contacted_count =
        persist_specialist_notifications_tx(tx, post, phase, &specialists).await?;

    sqlx::query(
        r#"
        INSERT INTO rescue_escalation_attempts (
          id, fanout_state_id, post_id, phase, strategy, radius_km,
          candidate_count, contacted_count, animal_scopes
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(state.id)
    .bind(state.post_id)
    .bind(phase.phase)
    .bind(phase.strategy)
    .bind(phase.radius_km)
    .bind(candidate_count)
    .bind(contacted_count)
    .bind(&scopes)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO rescue_fanout_attempts (
          id, fanout_state_id, post_id, phase, radius_km, candidate_count,
          push_jobs_created, confirmed_count_at_run, reason
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(state.id)
    .bind(state.post_id)
    .bind(phase.phase)
    .bind(phase.radius_km)
    .bind(candidate_count)
    .bind(contacted_count)
    .bind(phase.strategy)
    .execute(&mut **tx)
    .await?;

    let next_phase = phase.phase + 1;
    if escalation_phase_for(next_phase).is_some() {
        sqlx::query(
            r#"
            UPDATE rescue_fanout_states
            SET current_phase = $2,
                last_radius_km = $3,
                next_run_at = now() + ($4::text || ' seconds')::interval,
                attempts = attempts + 1,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(state.id)
        .bind(next_phase)
        .bind(phase.radius_km)
        .bind(phase.next_delay_seconds)
        .execute(&mut **tx)
        .await?;
    } else {
        let delay_seconds = seconds_until_soft_fanout(state.created_at);
        sqlx::query(
            r#"
            UPDATE rescue_fanout_states
            SET current_phase = $2,
                status = 'active',
                last_radius_km = $3,
                next_run_at = now() + ($4::text || ' seconds')::interval,
                attempts = attempts + 1,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(state.id)
        .bind(SOFT_FANOUT_PHASE)
        .bind(phase.radius_km)
        .bind(delay_seconds)
        .execute(&mut **tx)
        .await?;
        tracing::info!(
            fanout_state_id = %state.id,
            post_id = %state.post_id,
            delay_seconds,
            "rescue fanout entering softened hourly mode"
        );
    }

    Ok(())
}

async fn process_soft_fanout_phase(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &FanoutState,
    post: &Post,
) -> Result<(), sqlx::Error> {
    let age_hours = Utc::now()
        .signed_duration_since(state.created_at)
        .num_hours();
    if age_hours >= MAX_FANOUT_LIFETIME_HOURS {
        tracing::info!(
            fanout_state_id = %state.id,
            post_id = %state.post_id,
            age_hours,
            "rescue fanout exhausted after softened mode lifetime"
        );
        complete_state(tx, state.id, "exhausted").await?;
        return Ok(());
    }

    let phase = SOFT_FANOUT;
    let candidates = ranked_candidates(tx, post, phase).await?;
    let candidate_count = candidates.len() as i32;
    let alert = alert_for_candidates(post, phase, candidates)?;
    let recipient_count = alert.recipients.len() as i32;
    persist_rescue_alert_tx(tx, &alert).await?;

    sqlx::query(
        r#"
        INSERT INTO rescue_fanout_attempts (
          id, fanout_state_id, post_id, phase, radius_km, candidate_count,
          push_jobs_created, confirmed_count_at_run, reason
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, 0, 'softened_hourly_refresh')
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(state.id)
    .bind(state.post_id)
    .bind(phase.phase)
    .bind(phase.radius_km)
    .bind(candidate_count)
    .bind(recipient_count)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE rescue_fanout_states
        SET current_phase = $2,
            last_radius_km = $3,
            next_run_at = now() + ($4::text || ' seconds')::interval,
            attempts = attempts + 1,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(state.id)
    .bind(SOFT_FANOUT_PHASE)
    .bind(phase.radius_km)
    .bind(SOFT_FANOUT_INTERVAL_SECONDS)
    .execute(&mut **tx)
    .await?;

    tracing::info!(
        fanout_state_id = %state.id,
        post_id = %state.post_id,
        candidate_count,
        recipient_count,
        "softened rescue fanout cycle completed"
    );

    Ok(())
}

async fn persist_specialist_notifications_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post: &Post,
    phase: EscalationPhase,
    specialists: &[SpecialistCandidate],
) -> Result<i32, sqlx::Error> {
    let mut contacted = 0;
    for specialist in specialists {
        let Some(user_id) = specialist.user_id else {
            if specialist.contactable {
                contacted += 1;
            }
            continue;
        };
        let Some(push_token) = specialist.push_token.as_ref() else {
            if specialist.contactable {
                contacted += 1;
            }
            continue;
        };
        let Some(platform) = specialist.platform.as_ref() else {
            continue;
        };

        let event_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            INSERT INTO notification_events (
              user_id, kind, title, body, post_id, image_url, distance_km, critical,
              deeplink, dedupe_key, ttl_seconds, category, payload
            )
            VALUES ($1, 'rescue_escalation', $2, $3, $4, $5, $6, true, $7, $8, 1800, 'rescue', $9)
            ON CONFLICT DO NOTHING
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind("Helpin: apoio especializado necessario")
        .bind(format!(
            "{} em {}. Caso sem resposta local; toque para coordenar ajuda especializada.",
            post.name, post.neighborhood
        ))
        .bind(&post.id)
        .bind(post.image.as_deref())
        .bind(specialist.distance_km)
        .bind(format!("zoohelp://post/{}?action=specialist", post.id))
        .bind(format!(
            "rescue-escalation:{}:{}:{}",
            post.id, phase.phase, user_id
        ))
        .bind(serde_json::json!({
            "phase": phase.phase,
            "strategy": phase.strategy,
            "radiusKm": phase.radius_km,
            "providerId": specialist.provider_id.map(|id| id.to_string()),
            "providerName": specialist.name,
            "providerType": specialist.provider_type,
            "platform": platform_as_str(platform)
        }))
        .fetch_optional(&mut **tx)
        .await?;

        let Some(event_id) = event_id else {
            continue;
        };

        sqlx::query(
            r#"
            INSERT INTO push_delivery_jobs (
              notification_event_id, user_id, push_token, platform, payload
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(event_id)
        .bind(user_id)
        .bind(push_token)
        .bind(platform_as_str(platform))
        .bind(serde_json::json!({
            "title": "Helpin: apoio especializado necessario",
            "body": format!("{} em {} precisa de apoio especializado regional.", post.name, post.neighborhood),
            "deeplink": format!("zoohelp://post/{}?action=specialist", post.id),
            "postId": post.id,
            "critical": true,
            "phase": phase.phase,
            "strategy": phase.strategy
        }))
        .execute(&mut **tx)
        .await?;
        contacted += 1;
    }

    Ok(contacted)
}

async fn persist_rescue_alert_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    alert: &RescueAlert,
) -> Result<(), sqlx::Error> {
    if alert.recipients.is_empty() {
        sqlx::query(
            r#"
            INSERT INTO notification_events (
              kind, title, body, post_id, image_url, critical, deeplink,
              dedupe_key, ttl_seconds, category, payload
            )
            VALUES ('rescue_alert', $1, $2, $3, $4, $5, $6, $7, 900, 'rescue', $8)
            "#,
        )
        .bind(&alert.title)
        .bind(&alert.body)
        .bind(&alert.post_id)
        .bind(alert.image_url.as_deref())
        .bind(alert.critical)
        .bind(format!("zoohelp://post/{}", alert.post_id))
        .bind(format!("rescue:{}", alert.post_id))
        .bind(serde_json::json!({ "alertId": alert.id, "radiusKm": alert.radius_km }))
        .execute(&mut **tx)
        .await?;
        return Ok(());
    }

    for recipient in &alert.recipients {
        let user_id = Uuid::parse_str(&recipient.user_id).ok();
        let event_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            INSERT INTO notification_events (
              user_id, kind, title, body, post_id, image_url, distance_km, critical,
              deeplink, dedupe_key, ttl_seconds, category, payload
            )
            VALUES ($1, 'rescue_alert', $2, $3, $4, $5, $6, $7, $8, $9, 900, 'rescue', $10)
            ON CONFLICT DO NOTHING
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(&alert.title)
        .bind(&alert.body)
        .bind(&alert.post_id)
        .bind(alert.image_url.as_deref())
        .bind(recipient.distance_km)
        .bind(alert.critical)
        .bind(format!("zoohelp://post/{}", alert.post_id))
        .bind(format!("rescue:{}", alert.post_id))
        .bind(serde_json::json!({
            "alertId": alert.id,
            "platform": platform_as_str(&recipient.platform),
            "deliveryStatus": recipient.delivery_status,
            "radiusKm": alert.radius_km
        }))
        .fetch_optional(&mut **tx)
        .await?;

        let Some(event_id) = event_id else {
            continue;
        };

        sqlx::query(
            r#"
            INSERT INTO push_delivery_jobs (
              notification_event_id, user_id, push_token, platform, payload
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(event_id)
        .bind(user_id)
        .bind(&recipient.push_token)
        .bind(platform_as_str(&recipient.platform))
        .bind(serde_json::json!({
            "title": alert.title,
            "body": alert.body,
            "deeplink": format!("zoohelp://post/{}", alert.post_id),
            "postId": alert.post_id,
            "critical": alert.critical
        }))
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn load_post_for_fanout(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post_id: Uuid,
) -> Result<Post, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
          p.id::text AS id,
          p.post_type::text AS post_type,
          p.animal_type,
          COALESCE(p.name, 'Publicação') AS name,
          COALESCE(p.breed, '') AS breed,
          COALESCE(p.age, '') AS age,
          p.description,
          COALESCE(p.location_label, '') AS location,
          COALESCE(p.neighborhood, p.location_label, '') AS neighborhood,
          (
            SELECT pm.public_url
            FROM post_media pm
            WHERE pm.post_id = p.id
            ORDER BY pm.sort_order ASC, pm.created_at ASC
            LIMIT 1
          ) AS image,
          p.text_only,
          p.likes_count,
          p.comments_count,
          p.shares_count,
          p.urgent,
          p.rescue_status,
          p.resolved_at,
          p.created_at,
          p.contact,
          p.tags,
          p.latitude,
          p.longitude,
          p.geo_status,
          p.geo_source,
          p.route_public,
          u.id::text AS author_id,
          u.name AS author_name,
          u.avatar_url AS author_avatar,
          u.verified AS author_verified,
          u.account_type::text AS author_type
        FROM posts p
        JOIN users u ON u.id = p.author_id
        WHERE p.id = $1
        "#,
    )
    .bind(post_id)
    .fetch_one(&mut **tx)
    .await?;

    let author_type = match row.get::<&str, _>("author_type") {
        "ong" => AccountType::Ong,
        "vet" => AccountType::Vet,
        "admin" => AccountType::Admin,
        _ => AccountType::Person,
    };

    Ok(Post {
        id: row.get("id"),
        post_type: post_type_from_str(row.get::<&str, _>("post_type")),
        animal_type: animal_type_from_str(row.get::<&str, _>("animal_type")),
        name: row.get("name"),
        breed: row.get("breed"),
        age: row.get("age"),
        description: row.get("description"),
        location: row.get("location"),
        neighborhood: row.get("neighborhood"),
        image: row.get("image"),
        images: Vec::new(),
        text_only: row.get("text_only"),
        author: Author {
            id: row.get("author_id"),
            name: row.get("author_name"),
            avatar: row.get("author_avatar"),
            verified: row.get("author_verified"),
            account_type: author_type,
        },
        likes: row.get::<i32, _>("likes_count").max(0) as u32,
        liked_by_me: false,
        comments: row.get::<i32, _>("comments_count").max(0) as u32,
        shares: row.get::<i32, _>("shares_count").max(0) as u32,
        urgent: row.get("urgent"),
        rescue_status: row.get("rescue_status"),
        resolved_at: row
            .get::<Option<DateTime<Utc>>, _>("resolved_at")
            .map(|v| v.to_rfc3339()),
        created_at: row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
        contact: row.get("contact"),
        tags: row.get("tags"),
        latitude: row.get("latitude"),
        longitude: row.get("longitude"),
        geo_status: row.get("geo_status"),
        geo_source: row.get("geo_source"),
        route_public: row.get("route_public"),
        rescue_operational: None,
        rescue_final_report: None,
    })
}

fn confirmed_coords(post: &Post) -> Result<(f64, f64), sqlx::Error> {
    post.latitude
        .zip(post.longitude)
        .ok_or_else(|| sqlx::Error::Protocol("fanout requires confirmed post coordinates".into()))
}

async fn ranked_candidates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post: &Post,
    phase: FanoutPhase,
) -> Result<Vec<Candidate>, sqlx::Error> {
    let (lat, lng) = confirmed_coords(post)?;
    let scopes = animal_scopes_for_post(post);
    let lat_delta = phase.radius_km / EARTH_KM_PER_DEGREE;
    let lng_delta = longitude_delta_for_radius(lat, phase.radius_km);
    let rows = sqlx::query(
        r#"
        SELECT
          ps.user_id,
          ps.push_token,
          ps.platform,
          ps.lat,
          ps.lng,
          ps.radius_km,
          ps.critical_alerts,
          ps.updated_at,
          u.trust_score,
          u.verified,
          u.account_type::text AS account_type,
          COALESCE(vp.active, false) AS volunteer_active,
          COALESCE(vp.verified, false) AS volunteer_verified,
          COALESCE(vp.responses_count, 0)::bigint AS volunteer_responses_count,
          COALESCE(vp.arrived_count, 0)::bigint AS volunteer_arrived_count,
          vp.service_radius_km AS volunteer_service_radius_km,
          COALESCE(vp.animal_scopes, ARRAY[]::text[]) AS volunteer_animal_scopes,
          (
            SELECT count(*)::int
            FROM notification_events ne
            WHERE ne.user_id = ps.user_id
              AND ne.kind = 'rescue_alert'
              AND ne.created_at > now() - interval '30 minutes'
          ) AS recent_30m,
          (
            SELECT count(*)::int
            FROM notification_events ne
            WHERE ne.user_id = ps.user_id
              AND ne.kind = 'rescue_alert'
              AND ne.created_at > now() - interval '60 minutes'
          ) AS recent_60m
        FROM push_subscriptions ps
        JOIN users u ON u.id = ps.user_id
        LEFT JOIN volunteer_profiles vp ON vp.user_id = ps.user_id
        WHERE u.deleted_at IS NULL
          AND ps.invalidated_at IS NULL
          AND ps.updated_at > now() - ($1::int * interval '1 minute')
          AND ps.lat BETWEEN $2 AND $3
          AND ps.lng BETWEEN $4 AND $5
          AND NOT EXISTS (
            SELECT 1
            FROM notification_events ne
            WHERE ne.user_id = ps.user_id
              AND ne.dedupe_key = $6
          )
          AND (
            $7::boolean = false
            OR u.verified = true
            OR u.account_type IN ('ong', 'vet', 'admin')
          )
        "#,
    )
    .bind(ACTIVE_SUBSCRIPTION_MAX_AGE_MINUTES as i32)
    .bind(lat - lat_delta)
    .bind(lat + lat_delta)
    .bind(lng - lng_delta)
    .bind(lng + lng_delta)
    .bind(format!("rescue:{}", post.id))
    .bind(phase.verified_escalation)
    .fetch_all(&mut **tx)
    .await?;

    let mut candidates = Vec::new();
    for row in rows {
        let distance = haversine_km(lat, lng, row.get("lat"), row.get("lng"));
        let subscription_radius: f64 = row.get("radius_km");
        if distance > phase.radius_km.min(subscription_radius) {
            continue;
        }

        let recent_30m: i32 = row.get("recent_30m");
        let recent_60m: i32 = row.get("recent_60m");
        if recent_30m >= MAX_RECENT_RESCUE_ALERTS_30M || recent_60m >= MAX_RECENT_RESCUE_ALERTS_60M
        {
            continue;
        }

        let critical_alerts: bool = row.get("critical_alerts");
        let updated_at: DateTime<Utc> = row.get("updated_at");
        let account_type: String = row.get("account_type");
        let trust_score: i16 = row.get("trust_score");
        let verified: bool = row.get("verified");
        let volunteer_active: bool = row.get("volunteer_active");
        let volunteer_verified: bool = row.get("volunteer_verified");
        let volunteer_scopes: Vec<String> = row.get("volunteer_animal_scopes");
        let volunteer_scope_match = volunteer_active
            && (volunteer_scopes.iter().any(|scope| scope == "general")
                || volunteer_scopes.iter().any(|scope| scopes.contains(scope)));
        let volunteer_radius = row.get::<Option<f64>, _>("volunteer_service_radius_km");
        if volunteer_scope_match && volunteer_radius.is_some_and(|radius| distance > radius) {
            continue;
        }
        let volunteer_bonus = if volunteer_scope_match {
            12.0 + if volunteer_verified { 8.0 } else { 0.0 }
                + (row.get::<i64, _>("volunteer_responses_count") as f64).min(20.0) * 0.3
                + (row.get::<i64, _>("volunteer_arrived_count") as f64).min(20.0) * 0.5
        } else {
            0.0
        };
        let score = candidate_score(
            distance,
            phase.radius_km,
            updated_at,
            trust_score,
            &account_type,
            verified,
            critical_alerts,
            recent_30m,
            recent_60m,
        ) + volunteer_bonus;

        candidates.push(Candidate {
            user_id: row.get("user_id"),
            push_token: row.get("push_token"),
            platform: push_platform_from_str(row.get::<&str, _>("platform")),
            distance_km: round_distance_km(distance),
            score,
        });
    }

    candidates.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.distance_km.total_cmp(&b.distance_km))
    });
    candidates.truncate(MAX_CANDIDATES_PER_ATTEMPT);
    Ok(candidates)
}

async fn ranked_specialist_candidates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post: &Post,
    phase: EscalationPhase,
    scopes: &[String],
) -> Result<Vec<SpecialistCandidate>, sqlx::Error> {
    let (post_lat, post_lng) = confirmed_coords(post)?;
    let lat_delta = phase.radius_km / EARTH_KM_PER_DEGREE;
    let lng_delta = longitude_delta_for_radius(post_lat, phase.radius_km);
    let rows = sqlx::query(
        r#"
        SELECT
          sp.id,
          sp.user_id,
          sp.name,
          sp.provider_type,
          sp.lat,
          sp.lng,
          sp.service_radius_km::float8 AS service_radius_km,
          sp.phone,
          sp.email,
          sp.verified,
          ps.push_token,
          ps.platform,
          ps.updated_at,
          u.trust_score,
          u.account_type::text AS account_type,
          u.verified AS user_verified
        FROM rescue_specialist_providers sp
        LEFT JOIN users u ON u.id = sp.user_id AND u.deleted_at IS NULL
        LEFT JOIN LATERAL (
          SELECT push_token, platform, updated_at
          FROM push_subscriptions ps
          WHERE ps.user_id = sp.user_id
          ORDER BY ps.updated_at DESC
          LIMIT 1
        ) ps ON true
        WHERE sp.active = true
          AND sp.lat IS NOT NULL
          AND sp.lng IS NOT NULL
          AND sp.lat BETWEEN $1 AND $2
          AND sp.lng BETWEEN $3 AND $4
          AND (
            sp.animal_scopes && $5::text[]
            OR sp.animal_scopes @> ARRAY['general']::text[]
          )
        "#,
    )
    .bind(post_lat - lat_delta)
    .bind(post_lat + lat_delta)
    .bind(post_lng - lng_delta)
    .bind(post_lng + lng_delta)
    .bind(scopes)
    .fetch_all(&mut **tx)
    .await?;

    let mut candidates = Vec::new();
    for row in rows {
        let lat: f64 = row.get("lat");
        let lng: f64 = row.get("lng");
        let distance = haversine_km(post_lat, post_lng, lat, lng);
        let service_radius: f64 = row.get("service_radius_km");
        if distance > phase.radius_km || distance > service_radius {
            continue;
        }

        let provider_type: String = row.get("provider_type");
        let trust_score = row.get::<Option<i16>, _>("trust_score").unwrap_or(50);
        let account_type = row
            .get::<Option<String>, _>("account_type")
            .unwrap_or_else(|| provider_type.clone());
        let verified = row.get::<bool, _>("verified")
            || row.get::<Option<bool>, _>("user_verified").unwrap_or(false);
        let push_token = row.get::<Option<String>, _>("push_token");
        let platform = row
            .get::<Option<String>, _>("platform")
            .as_deref()
            .map(push_platform_from_str);
        let updated_at = row.get::<Option<DateTime<Utc>>, _>("updated_at");
        let score = specialist_score(
            distance,
            phase.radius_km,
            trust_score,
            &account_type,
            &provider_type,
            verified,
            updated_at,
        );

        candidates.push(SpecialistCandidate {
            provider_id: row.get("id"),
            user_id: row.get("user_id"),
            name: row.get("name"),
            provider_type,
            push_token,
            platform,
            distance_km: round_distance_km(distance),
            score,
            contactable: row.get::<Option<String>, _>("phone").is_some()
                || row.get::<Option<String>, _>("email").is_some(),
        });
    }

    if candidates.is_empty() {
        candidates = ranked_verified_regional_fallback(tx, post, phase).await?;
    }

    candidates.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.distance_km.total_cmp(&b.distance_km))
    });
    candidates.truncate(MAX_SPECIALIST_CANDIDATES_PER_ATTEMPT);
    Ok(candidates)
}

async fn ranked_verified_regional_fallback(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post: &Post,
    phase: EscalationPhase,
) -> Result<Vec<SpecialistCandidate>, sqlx::Error> {
    let (post_lat, post_lng) = confirmed_coords(post)?;
    let capped_radius = phase.radius_km.min(50.0);
    let lat_delta = capped_radius / EARTH_KM_PER_DEGREE;
    let lng_delta = longitude_delta_for_radius(post_lat, capped_radius);
    let rows = sqlx::query(
        r#"
        SELECT
          ps.user_id,
          ps.push_token,
          ps.platform,
          ps.lat,
          ps.lng,
          ps.updated_at,
          u.name,
          u.trust_score,
          u.verified,
          u.account_type::text AS account_type,
          (
            SELECT count(*)::int
            FROM notification_events ne
            WHERE ne.user_id = ps.user_id
              AND ne.kind IN ('rescue_alert', 'rescue_escalation')
              AND ne.created_at > now() - interval '60 minutes'
          ) AS recent_60m
        FROM push_subscriptions ps
        JOIN users u ON u.id = ps.user_id
        WHERE u.deleted_at IS NULL
          AND ps.invalidated_at IS NULL
          AND ps.updated_at > now() - ($6::int * interval '1 minute')
          AND ps.lat BETWEEN $1 AND $2
          AND ps.lng BETWEEN $3 AND $4
          AND (u.verified = true OR u.account_type IN ('ong', 'vet', 'admin'))
          AND NOT EXISTS (
            SELECT 1
            FROM notification_events ne
            WHERE ne.user_id = ps.user_id
              AND ne.dedupe_key = $5
          )
        "#,
    )
    .bind(post_lat - lat_delta)
    .bind(post_lat + lat_delta)
    .bind(post_lng - lng_delta)
    .bind(post_lng + lng_delta)
    .bind(format!("rescue-escalation:{}:{}", post.id, phase.phase))
    .bind(ACTIVE_SUBSCRIPTION_MAX_AGE_MINUTES as i32)
    .fetch_all(&mut **tx)
    .await?;

    let mut candidates = Vec::new();
    for row in rows {
        let distance = haversine_km(post_lat, post_lng, row.get("lat"), row.get("lng"));
        if distance > capped_radius
            || row.get::<i32, _>("recent_60m") >= MAX_RECENT_RESCUE_ALERTS_60M
        {
            continue;
        }
        let account_type: String = row.get("account_type");
        let verified: bool = row.get("verified");
        let trust_score: i16 = row.get("trust_score");
        let updated_at: DateTime<Utc> = row.get("updated_at");
        let score = specialist_score(
            distance,
            capped_radius,
            trust_score,
            &account_type,
            "verified_fallback",
            verified,
            Some(updated_at),
        );
        candidates.push(SpecialistCandidate {
            provider_id: None,
            user_id: row.get("user_id"),
            name: row.get("name"),
            provider_type: account_type,
            push_token: row.get("push_token"),
            platform: Some(push_platform_from_str(row.get::<&str, _>("platform"))),
            distance_km: round_distance_km(distance),
            score,
            contactable: true,
        });
    }
    Ok(candidates)
}

fn alert_for_candidates(
    post: &Post,
    phase: FanoutPhase,
    candidates: Vec<Candidate>,
) -> Result<RescueAlert, sqlx::Error> {
    let (lat, lng) = confirmed_coords(post)?;
    Ok(RescueAlert {
        id: Uuid::now_v7().to_string(),
        post_id: post.id.clone(),
        title: if post.urgent {
            "Resgate urgente perto de voce".to_string()
        } else {
            "Animal precisa de ajuda perto de voce".to_string()
        },
        body: format!(
            "{} em {}. Toque para confirmar ajuda, abrir rota ou coordenar pelo chat.",
            post.name, post.neighborhood
        ),
        image_url: post.image.clone(),
        lat,
        lng,
        radius_km: phase.radius_km,
        critical: post.urgent,
        actions: vec![
            NotificationAction {
                id: "confirm_going",
                label: "Estou indo",
                deep_link: format!("zoohelp://post/{}?action=going", post.id),
            },
            NotificationAction {
                id: "remote_support",
                label: "Apoiar remoto",
                deep_link: format!("zoohelp://post/{}?action=chat", post.id),
            },
        ],
        recipient_count: candidates.len(),
        recipients: candidates
            .into_iter()
            .map(|candidate| AlertRecipient {
                user_id: candidate.user_id.to_string(),
                push_token: candidate.push_token,
                platform: candidate.platform,
                distance_km: candidate.distance_km,
                delivery_status: "queued",
            })
            .collect(),
        created_at: Utc::now().to_rfc3339(),
    })
}

fn candidate_score(
    distance_km: f64,
    radius_km: f64,
    updated_at: DateTime<Utc>,
    trust_score: i16,
    account_type: &str,
    verified: bool,
    critical_alerts: bool,
    recent_30m: i32,
    recent_60m: i32,
) -> f64 {
    let distance_score = (1.0 - (distance_km / radius_km).clamp(0.0, 1.0)) * 60.0;
    let age_minutes = (Utc::now() - updated_at).num_minutes();
    let recent_activity_score = if age_minutes <= 5 {
        15.0
    } else if age_minutes <= 15 {
        10.0
    } else {
        0.0
    };
    let trust_score = (trust_score as f64).clamp(0.0, 100.0) / 5.0;
    let role_bonus = match account_type {
        "ong" | "vet" => 20.0,
        "admin" => 12.0,
        _ => 0.0,
    };
    let verified_bonus = if verified { 10.0 } else { 0.0 };
    let critical_bonus = if critical_alerts { 8.0 } else { 0.0 };
    let fatigue_penalty = recent_30m as f64 * 12.0 + recent_60m as f64 * 5.0;

    distance_score
        + recent_activity_score
        + trust_score
        + role_bonus
        + verified_bonus
        + critical_bonus
        - fatigue_penalty
}

fn specialist_score(
    distance_km: f64,
    radius_km: f64,
    trust_score: i16,
    account_type: &str,
    provider_type: &str,
    verified: bool,
    updated_at: Option<DateTime<Utc>>,
) -> f64 {
    let distance_score = (1.0 - (distance_km / radius_km).clamp(0.0, 1.0)) * 35.0;
    let trust_score = (trust_score as f64).clamp(0.0, 100.0) / 4.0;
    let provider_bonus = match provider_type {
        "cetas" | "ibama" | "environmental_police" => 35.0,
        "wildlife_rescue" | "marine_rescue" | "rural_rescue" => 28.0,
        "ong" | "vet" => 22.0,
        _ => 8.0,
    };
    let role_bonus = match account_type {
        "ong" | "vet" | "admin" => 10.0,
        _ => 0.0,
    };
    let verified_bonus = if verified { 15.0 } else { 0.0 };
    let recent_bonus = updated_at
        .map(|updated_at| {
            let age_minutes = (Utc::now() - updated_at).num_minutes();
            if age_minutes <= 15 {
                12.0
            } else if age_minutes <= 60 {
                6.0
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    distance_score + trust_score + provider_bonus + role_bonus + verified_bonus + recent_bonus
}

fn animal_scopes_for_post(post: &Post) -> Vec<String> {
    let haystack = format!(
        "{} {} {} {}",
        post.name,
        post.description,
        post.breed,
        post.tags.join(" ")
    )
    .to_lowercase();

    let mut scopes = vec!["general".to_string()];
    match post.animal_type {
        AnimalType::Dog => scopes.push("dog".to_string()),
        AnimalType::Cat => scopes.push("cat".to_string()),
        AnimalType::Bird => scopes.push("bird".to_string()),
        AnimalType::Wildlife => scopes.push("wildlife".to_string()),
        AnimalType::Reptile => scopes.push("reptile".to_string()),
        AnimalType::Livestock => scopes.push("livestock".to_string()),
        AnimalType::Marine => scopes.push("marine".to_string()),
        AnimalType::Other => scopes.push("other".to_string()),
    }

    for (scope, needles) in [
        (
            "wildlife",
            &[
                "silvestre",
                "selvagem",
                "macaco",
                "mico",
                "gambá",
                "gamba",
                "capivara",
                "tamanduá",
                "tamandua",
                "cervo",
                "veado",
                "jacaré",
                "jacare",
                "cobra",
                "lagarto",
                "raposa",
            ][..],
        ),
        (
            "bird",
            &[
                "ave", "passaro", "pássaro", "coruja", "gavião", "gaviao", "papagaio", "arara",
                "pombo",
            ][..],
        ),
        (
            "marine",
            &[
                "marinho",
                "praia",
                "oleado",
                "tartaruga",
                "pinguim",
                "golfinho",
                "baleia",
            ][..],
        ),
        (
            "livestock",
            &[
                "cavalo", "égua", "egua", "boi", "vaca", "bezerro", "burro", "mula", "porco",
            ][..],
        ),
        (
            "reptile",
            &["jacaré", "jacare", "cobra", "serpente", "lagarto", "iguana"][..],
        ),
    ] {
        if needles.iter().any(|needle| haystack.contains(needle)) {
            scopes.push(scope.to_string());
        }
    }

    scopes.sort();
    scopes.dedup();
    scopes
}

async fn count_responses(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post_id: Uuid,
) -> Result<(i32, i32), sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
          count(*) FILTER (WHERE status = 'confirmed')::int AS confirmed_count,
          count(*) FILTER (WHERE status = 'arrived')::int AS arrived_count
        FROM rescue_responses
        WHERE post_id = $1
          AND action = 'going'
          AND status IN ('confirmed', 'arrived')
        "#,
    )
    .bind(post_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok((row.get("confirmed_count"), row.get("arrived_count")))
}

async fn refresh_fanout_response_counts(db: &PgPool, post_id: Uuid) -> Result<(), sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
          count(*) FILTER (WHERE status = 'confirmed')::int AS confirmed_count,
          count(*) FILTER (WHERE status = 'arrived')::int AS arrived_count
        FROM rescue_responses
        WHERE post_id = $1
          AND action = 'going'
          AND status IN ('confirmed', 'arrived')
        "#,
    )
    .bind(post_id)
    .fetch_one(db)
    .await?;

    sqlx::query(
        r#"
        UPDATE rescue_fanout_states
        SET confirmed_count = $2,
            arrived_count = $3,
            updated_at = now()
        WHERE post_id = $1
        "#,
    )
    .bind(post_id)
    .bind(row.get::<i32, _>("confirmed_count"))
    .bind(row.get::<i32, _>("arrived_count"))
    .execute(db)
    .await?;
    Ok(())
}

async fn pause_fanout_if_confirmed(db: &PgPool, post_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE rescue_fanout_states
        SET status = 'paused',
            updated_at = now()
        WHERE post_id = $1
          AND status = 'active'
          AND confirmed_count > 0
        "#,
    )
    .bind(post_id)
    .execute(db)
    .await?;
    Ok(())
}

async fn update_state_counts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    confirmed_count: i32,
    arrived_count: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE rescue_fanout_states
        SET confirmed_count = $2,
            arrived_count = $3,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(confirmed_count)
    .bind(arrived_count)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn complete_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE rescue_fanout_states
        SET status = $2,
            updated_at = now(),
            completed_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(status)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn phase_for(current_phase: i32) -> Option<FanoutPhase> {
    FANOUT_PHASES
        .iter()
        .copied()
        .find(|phase| phase.phase == current_phase)
}

fn escalation_phase_for(current_phase: i32) -> Option<EscalationPhase> {
    ESCALATION_PHASES
        .iter()
        .copied()
        .find(|phase| phase.phase == current_phase)
}

fn seconds_until_soft_fanout(created_at: DateTime<Utc>) -> i64 {
    let elapsed = Utc::now()
        .signed_duration_since(created_at)
        .num_seconds()
        .max(0);
    let soft_after = SOFT_FANOUT_AFTER_MINUTES * 60;
    if elapsed >= soft_after {
        SOFT_FANOUT_INTERVAL_SECONDS
    } else {
        (soft_after - elapsed).max(60)
    }
}

fn longitude_delta_for_radius(lat: f64, radius_km: f64) -> f64 {
    let latitude_factor = lat.to_radians().cos().abs().max(0.01);
    radius_km / (EARTH_KM_PER_DEGREE * latitude_factor)
}

fn round_distance_km(distance: f64) -> f64 {
    (distance * 1000.0).round() / 1000.0
}

fn platform_as_str(platform: &PushPlatform) -> &'static str {
    match platform {
        PushPlatform::Ios => "ios",
        PushPlatform::Android => "android",
        PushPlatform::Expo => "expo",
        PushPlatform::Web => "web",
    }
}

fn post_type_from_str(value: &str) -> PostType {
    match value {
        "adoption" => PostType::Adoption,
        "lost" => PostType::Lost,
        "found" => PostType::Found,
        "emergency" => PostType::Emergency,
        "campaign" => PostType::Campaign,
        _ => PostType::Post,
    }
}

fn animal_type_from_str(value: &str) -> AnimalType {
    match value {
        "dog" => AnimalType::Dog,
        "cat" => AnimalType::Cat,
        "bird" => AnimalType::Bird,
        "wildlife" => AnimalType::Wildlife,
        "reptile" => AnimalType::Reptile,
        "livestock" => AnimalType::Livestock,
        "marine" => AnimalType::Marine,
        _ => AnimalType::Other,
    }
}

#[allow(dead_code)]
pub async fn dispatch_phase_one_now_for_tests(
    db: &PgPool,
    post_id: Uuid,
) -> Result<(), sqlx::Error> {
    let state_id = create_fanout_state_for_post(db, post_id, None).await?;
    process_one_fanout(db, state_id).await
}
