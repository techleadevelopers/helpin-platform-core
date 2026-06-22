use anyhow::Context;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::{env, time::Instant};
use uuid::Uuid;

const SEED_TAG: &str = "rescue_fanout_test";
const PASSWORD: &str = "Helpin@123456";
const BASE_LAT: f64 = -23.561684;
const BASE_LNG: f64 = -46.655981;

const AUTHOR_ID: &str = "77777777-7777-7777-8777-777777777701";
const NEAR_100_ID: &str = "77777777-7777-7777-8777-777777777702";
const NEAR_500_ID: &str = "77777777-7777-7777-8777-777777777703";
const NEAR_900_ID: &str = "77777777-7777-7777-8777-777777777704";
const FAR_5K_ID: &str = "77777777-7777-7777-8777-777777777705";
const NO_PUSH_ID: &str = "77777777-7777-7777-8777-777777777706";
const INVALID_PUSH_ID: &str = "77777777-7777-7777-8777-777777777707";
const ONG_USER_ID: &str = "77777777-7777-7777-8777-777777777708";
const ONG_ID: &str = "77777777-7777-7777-8777-777777777709";
const POST_ID: &str = "77777777-7777-7777-8777-777777777710";
const RESCUE_ID: &str = "77777777-7777-7777-8777-777777777711";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeedMode {
    Fake,
    RealDevice,
}

impl SeedMode {
    fn from_env() -> anyhow::Result<Self> {
        let raw = env::var("SEED_MODE").unwrap_or_else(|_| "fake".to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "fake" => Ok(Self::Fake),
            "real_device" | "real-device" | "real" => Ok(Self::RealDevice),
            other => anyhow::bail!("SEED_MODE must be fake or real_device, got {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Fake => "fake",
            Self::RealDevice => "real_device",
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .context("failed to connect to database")?;

    let command = env::args().nth(1).unwrap_or_else(|| "seed".to_string());
    match command.as_str() {
        "seed" => {
            reset_seed(&pool).await?;
            seed(&pool).await?;
            report(&pool).await?;
        }
        "simulate-response" => {
            simulate_response_and_chat(&pool).await?;
            report(&pool).await?;
        }
        "reset" => reset_seed(&pool).await?,
        "report" => report(&pool).await?,
        other => anyhow::bail!(
            "unknown command '{other}', use seed, simulate-response, reset, or report"
        ),
    }

    Ok(())
}

async fn seed(pool: &PgPool) -> anyhow::Result<()> {
    let started = Instant::now();
    let mode = SeedMode::from_env()?;
    let real_push_token = env::var("REAL_PUSH_TOKEN")
        .or_else(|_| env::var("HELPIN_REAL_PUSH_TOKEN"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if mode == SeedMode::RealDevice && real_push_token.is_none() {
        anyhow::bail!("SEED_MODE=real_device requires REAL_PUSH_TOKEN=ExponentPushToken[...]");
    }
    let real_platform = env::var("REAL_PUSH_PLATFORM")
        .or_else(|_| env::var("HELPIN_REAL_PUSH_PLATFORM"))
        .unwrap_or_else(|_| "android".to_string())
        .to_ascii_lowercase();
    let near_100_token = real_push_token
        .clone()
        .unwrap_or_else(|| "ExponentPushToken[rescue-fanout-test-near100]".to_string());

    upsert_user(
        pool,
        AUTHOR_ID,
        "[seed rescue] Autor do caso",
        "rescue_fanout_test+author@helpin.local",
        "person",
        true,
        80,
    )
    .await?;
    upsert_user(
        pool,
        NEAR_100_ID,
        "[seed rescue] Protetor 100m",
        "rescue_fanout_test+near100@helpin.local",
        "person",
        true,
        85,
    )
    .await?;
    upsert_user(
        pool,
        NEAR_500_ID,
        "[seed rescue] Protetor 500m",
        "rescue_fanout_test+near500@helpin.local",
        "person",
        true,
        82,
    )
    .await?;
    upsert_user(
        pool,
        NEAR_900_ID,
        "[seed rescue] Protetor 900m",
        "rescue_fanout_test+near900@helpin.local",
        "person",
        true,
        79,
    )
    .await?;
    upsert_user(
        pool,
        FAR_5K_ID,
        "[seed rescue] Protetor 5km",
        "rescue_fanout_test+far5km@helpin.local",
        "person",
        true,
        70,
    )
    .await?;
    upsert_user(
        pool,
        NO_PUSH_ID,
        "[seed rescue] Usuario sem push",
        "rescue_fanout_test+nopush@helpin.local",
        "person",
        true,
        75,
    )
    .await?;
    upsert_user(
        pool,
        INVALID_PUSH_ID,
        "[seed rescue] Token invalido",
        "rescue_fanout_test+invalidpush@helpin.local",
        "person",
        true,
        76,
    )
    .await?;
    upsert_user(
        pool,
        ONG_USER_ID,
        "[seed rescue] ONG verificada",
        "rescue_fanout_test+ong@helpin.local",
        "ong",
        true,
        95,
    )
    .await?;
    upsert_ong(pool).await?;

    upsert_push_subscription(
        pool,
        NEAR_100_ID,
        &near_100_token,
        &real_platform,
        BASE_LAT + 0.0009,
        BASE_LNG,
        8.0,
    )
    .await?;
    upsert_push_subscription(
        pool,
        NEAR_500_ID,
        "ExponentPushToken[rescue-fanout-test-near500]",
        "expo",
        BASE_LAT + 0.0045,
        BASE_LNG,
        8.0,
    )
    .await?;
    upsert_push_subscription(
        pool,
        NEAR_900_ID,
        "ExponentPushToken[rescue-fanout-test-near900]",
        "expo",
        BASE_LAT + 0.0081,
        BASE_LNG,
        8.0,
    )
    .await?;
    upsert_push_subscription(
        pool,
        FAR_5K_ID,
        "ExponentPushToken[rescue-fanout-test-far5km]",
        "expo",
        BASE_LAT + 0.0450,
        BASE_LNG,
        8.0,
    )
    .await?;
    upsert_push_subscription(
        pool,
        INVALID_PUSH_ID,
        "ExponentPushToken[rescue-fanout-test-invalid]",
        "expo",
        BASE_LAT + 0.0007,
        BASE_LNG,
        8.0,
    )
    .await?;
    upsert_push_subscription(
        pool,
        ONG_USER_ID,
        "ExponentPushToken[rescue-fanout-test-ong]",
        "expo",
        BASE_LAT + 0.0200,
        BASE_LNG,
        50.0,
    )
    .await?;

    upsert_post(pool).await?;
    upsert_rescue_session(pool).await?;
    upsert_fanout_state(pool).await?;

    println!("seed_tag: {SEED_TAG}");
    println!("seed_mode: {}", mode.as_str());
    println!("post_id: {POST_ID}");
    println!("rescue_id: {RESCUE_ID}");
    println!("real_push_token_attached_to: rescue_fanout_test+near100@helpin.local");
    println!("real_push_token_enabled: {}", real_push_token.is_some());
    println!("seed_elapsed_ms: {}", started.elapsed().as_millis());
    println!("next: run backend with RESCUE_FANOUT_WORKER_ENABLED=true and PUSH_WORKER_ENABLED=true, then run `cargo run --bin seed_rescue_fanout_test report`");

    Ok(())
}

async fn reset_seed(pool: &PgPool) -> anyhow::Result<()> {
    let seed_user_ids = seed_user_ids()?;
    let post_id = Uuid::parse_str(POST_ID)?;
    let rescue_id = Uuid::parse_str(RESCUE_ID)?;

    sqlx::query("DELETE FROM push_delivery_jobs WHERE notification_event_id IN (SELECT id FROM notification_events WHERE post_id = $1 OR dedupe_key LIKE $2)")
        .bind(POST_ID)
        .bind(format!("%{POST_ID}%"))
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM notification_events WHERE post_id = $1 OR dedupe_key LIKE $2")
        .bind(POST_ID)
        .bind(format!("%{POST_ID}%"))
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rescue_responses WHERE post_id = $1 OR rescue_session_id = $2")
        .bind(post_id)
        .bind(rescue_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rescue_events WHERE post_id = $1 OR rescue_id = $2")
        .bind(post_id)
        .bind(rescue_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rescue_fanout_attempts WHERE post_id = $1")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rescue_escalation_attempts WHERE post_id = $1")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rescue_fanout_states WHERE post_id = $1")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rescue_final_reports WHERE post_id = $1")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rescue_sessions WHERE id = $1 OR post_id = $2")
        .bind(rescue_id)
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM chat_room_members WHERE room_id IN (SELECT id FROM chat_rooms WHERE post_id = $1)")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM chat_messages WHERE room_id IN (SELECT id FROM chat_rooms WHERE post_id = $1)",
    )
    .bind(post_id)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM chat_rooms WHERE post_id = $1")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM post_media WHERE post_id = $1")
        .bind(post_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM posts WHERE id = $1 OR $2 = ANY(tags) OR $3 = ANY(tags)")
        .bind(post_id)
        .bind(SEED_TAG)
        .bind(format!("#{SEED_TAG}"))
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM push_subscriptions WHERE user_id = ANY($1) OR push_token LIKE 'ExponentPushToken[rescue-fanout-test%'")
        .bind(&seed_user_ids)
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM ong_profiles WHERE id = $1 OR user_id = ANY($2) OR cnpj = '77999999000177'",
    )
    .bind(Uuid::parse_str(ONG_ID)?)
    .bind(&seed_user_ids)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM users WHERE id = ANY($1) OR email::text LIKE 'rescue_fanout_test+%@helpin.local'")
        .bind(&seed_user_ids)
        .execute(pool)
        .await?;

    println!("reset_seed: ok");
    Ok(())
}

async fn upsert_user(
    pool: &PgPool,
    id: &str,
    name: &str,
    email: &str,
    account_type: &str,
    verified: bool,
    trust_score: i16,
) -> anyhow::Result<()> {
    let password_hash = hash_password(PASSWORD)?;
    sqlx::query(
        r#"
        INSERT INTO users (
          id, name, email, avatar_url, password_hash, account_type, verified,
          trust_score, city, state, deleted_at, anonymized_at, retention_delete_after
        )
        VALUES ($1, $2, $3, NULL, $4, $5::account_type, $6, $7, 'Sao Paulo', 'SP', NULL, NULL, NULL)
        ON CONFLICT (email) DO UPDATE SET
          id = EXCLUDED.id,
          name = EXCLUDED.name,
          password_hash = EXCLUDED.password_hash,
          account_type = EXCLUDED.account_type,
          verified = EXCLUDED.verified,
          trust_score = EXCLUDED.trust_score,
          city = EXCLUDED.city,
          state = EXCLUDED.state,
          deleted_at = NULL,
          anonymized_at = NULL,
          retention_delete_after = NULL
        "#,
    )
    .bind(Uuid::parse_str(id)?)
    .bind(name)
    .bind(email)
    .bind(password_hash)
    .bind(account_type)
    .bind(verified)
    .bind(trust_score)
    .execute(pool)
    .await?;
    Ok(())
}

async fn upsert_ong(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO ong_profiles (
          id, user_id, legal_name, cnpj, mission, city, state, latitude, longitude,
          verified_at, area_type, contact_phone, verification_status, verification_reviewed_at
        )
        VALUES ($1, $2, 'ONG Seed Verificada', '77999999000177',
          'Seed operacional para validar fanout de resgate.', 'Sao Paulo', 'SP',
          $3, $4, now(), 'rescue', '(11) 99999-0000', 'APPROVED', now())
        ON CONFLICT (cnpj) DO UPDATE SET
          user_id = EXCLUDED.user_id,
          legal_name = EXCLUDED.legal_name,
          latitude = EXCLUDED.latitude,
          longitude = EXCLUDED.longitude,
          verified_at = now(),
          verification_status = 'APPROVED',
          verification_reviewed_at = now()
        "#,
    )
    .bind(Uuid::parse_str(ONG_ID)?)
    .bind(Uuid::parse_str(ONG_USER_ID)?)
    .bind(BASE_LAT + 0.0200)
    .bind(BASE_LNG)
    .execute(pool)
    .await?;
    Ok(())
}

async fn upsert_push_subscription(
    pool: &PgPool,
    user_id: &str,
    push_token: &str,
    platform: &str,
    lat: f64,
    lng: f64,
    radius_km: f64,
) -> anyhow::Result<()> {
    let platform = match platform {
        "ios" | "android" | "expo" | "web" => platform,
        _ => "expo",
    };
    sqlx::query(
        r#"
        INSERT INTO push_subscriptions (
          user_id, push_token, platform, lat, lng, radius_km, critical_alerts, invalidated_at, last_delivery_error, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, true, NULL, NULL, now())
        ON CONFLICT (push_token) DO UPDATE SET
          user_id = EXCLUDED.user_id,
          platform = EXCLUDED.platform,
          lat = EXCLUDED.lat,
          lng = EXCLUDED.lng,
          radius_km = EXCLUDED.radius_km,
          critical_alerts = true,
          invalidated_at = NULL,
          last_delivery_error = NULL,
          updated_at = now()
        "#,
    )
    .bind(Uuid::parse_str(user_id)?)
    .bind(push_token)
    .bind(platform)
    .bind(lat)
    .bind(lng)
    .bind(radius_km)
    .execute(pool)
    .await?;
    Ok(())
}

async fn upsert_post(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO posts (
          id, author_id, post_type, animal_type, name, breed, age, description,
          latitude, longitude, location_label, neighborhood, contact, tags,
          urgent, rescue_status, text_only, moderation_status, fraud_risk,
          geo_status, geo_source, route_public, geo_provider, geo_confidence, geo_resolved_at
        )
        VALUES (
          $1, $2, 'emergency'::post_type, 'dog', '[seed rescue] cachorro ferido proximo', '', '',
          'Seed operacional: animal ferido para validar fanout, push, resposta, chat e status.',
          $3, $4, 'Avenida Paulista, Bela Vista, Sao Paulo, SP', 'Bela Vista', '',
          ARRAY[$5, $6]::text[], true, 'active', true, 'approved', 0,
          'confirmed', 'gps_confirmed', true, 'device', 1.0, now()
        )
        ON CONFLICT (id) DO UPDATE SET
          author_id = EXCLUDED.author_id,
          latitude = EXCLUDED.latitude,
          longitude = EXCLUDED.longitude,
          urgent = true,
          rescue_status = 'active',
          geo_status = 'confirmed',
          geo_source = 'gps_confirmed',
          route_public = true,
          tags = EXCLUDED.tags,
          resolved_at = NULL
        "#,
    )
    .bind(Uuid::parse_str(POST_ID)?)
    .bind(Uuid::parse_str(AUTHOR_ID)?)
    .bind(BASE_LAT)
    .bind(BASE_LNG)
    .bind(SEED_TAG)
    .bind(format!("#{SEED_TAG}"))
    .execute(pool)
    .await?;
    Ok(())
}

async fn upsert_rescue_session(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO rescue_sessions (id, post_id, reporter_user_id, status, lat, lng, accuracy)
        VALUES ($1, $2, $3, 'active', $4, $5, 20)
        ON CONFLICT (id) DO UPDATE SET
          post_id = EXCLUDED.post_id,
          reporter_user_id = EXCLUDED.reporter_user_id,
          status = 'active',
          lat = EXCLUDED.lat,
          lng = EXCLUDED.lng,
          accuracy = EXCLUDED.accuracy,
          ended_at = NULL,
          updated_at = now()
        "#,
    )
    .bind(Uuid::parse_str(RESCUE_ID)?)
    .bind(Uuid::parse_str(POST_ID)?)
    .bind(Uuid::parse_str(AUTHOR_ID)?)
    .bind(BASE_LAT)
    .bind(BASE_LNG)
    .execute(pool)
    .await?;
    Ok(())
}

async fn upsert_fanout_state(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO rescue_fanout_states (
          post_id, rescue_session_id, current_phase, status, last_radius_km, next_run_at,
          confirmed_count, arrived_count, attempts
        )
        VALUES ($1, $2, 1, 'active', 0.3, now(), 0, 0, 0)
        ON CONFLICT (post_id) DO UPDATE SET
          rescue_session_id = EXCLUDED.rescue_session_id,
          current_phase = 1,
          status = 'active',
          last_radius_km = 0.3,
          next_run_at = now(),
          confirmed_count = 0,
          arrived_count = 0,
          attempts = 0,
          completed_at = NULL,
          updated_at = now()
        "#,
    )
    .bind(Uuid::parse_str(POST_ID)?)
    .bind(Uuid::parse_str(RESCUE_ID)?)
    .execute(pool)
    .await?;
    Ok(())
}

async fn report(pool: &PgPool) -> anyhow::Result<()> {
    let post_id = Uuid::parse_str(POST_ID)?;
    let rows = sqlx::query(
        r#"
        SELECT
          u.email::text AS email,
          u.account_type::text AS account_type,
          u.verified,
          ps.push_token,
          CASE
            WHEN ps.user_id IS NULL THEN NULL
            ELSE round((6371 * acos(
              LEAST(1, GREATEST(-1,
                cos(radians($2)) * cos(radians(ps.lat)) *
                cos(radians(ps.lng) - radians($3)) +
                sin(radians($2)) * sin(radians(ps.lat))
              ))
            ))::numeric, 3)::text
          END AS distance_km,
          ps.radius_km,
          COALESCE(ps.invalidated_at IS NULL, false) AS active,
          CASE
            WHEN ps.user_id IS NULL THEN 'no_push_token'
            WHEN (6371 * acos(LEAST(1, GREATEST(-1,
              cos(radians($2)) * cos(radians(ps.lat)) *
              cos(radians(ps.lng) - radians($3)) +
              sin(radians($2)) * sin(radians(ps.lat))
            )))) <= 0.3 THEN 'phase_1_should_receive'
            WHEN (6371 * acos(LEAST(1, GREATEST(-1,
              cos(radians($2)) * cos(radians(ps.lat)) *
              cos(radians(ps.lng) - radians($3)) +
              sin(radians($2)) * sin(radians(ps.lat))
            )))) <= 0.7 THEN 'phase_2_should_receive'
            WHEN (6371 * acos(LEAST(1, GREATEST(-1,
              cos(radians($2)) * cos(radians(ps.lat)) *
              cos(radians(ps.lng) - radians($3)) +
              sin(radians($2)) * sin(radians(ps.lat))
            )))) <= 1.0 THEN 'phase_3_should_receive'
            WHEN u.account_type::text = 'ong' OR u.verified = true THEN 'later_or_verified_escalation'
            ELSE 'should_not_receive_initially'
          END AS expected
        FROM users u
        LEFT JOIN push_subscriptions ps ON ps.user_id = u.id
        WHERE u.id = ANY($1)
          AND u.id <> $4
        ORDER BY ps.user_id IS NULL, distance_km NULLS LAST, email ASC
        "#,
    )
    .bind(&seed_user_ids()?)
    .bind(BASE_LAT)
    .bind(BASE_LNG)
    .bind(Uuid::parse_str(AUTHOR_ID)?)
    .fetch_all(pool)
    .await?;

    println!("candidates:");
    for row in rows {
        let token = row
            .get::<Option<String>, _>("push_token")
            .map(|value| mask_token(&value))
            .unwrap_or_else(|| "none".to_string());
        let distance = row
            .get::<Option<String>, _>("distance_km")
            .unwrap_or_else(|| "none".to_string());
        let radius = row
            .get::<Option<f64>, _>("radius_km")
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string());
        println!(
            "- {} type={} verified={} distance_km={} radius_km={} active={} expected={} token={}",
            row.get::<String, _>("email"),
            row.get::<String, _>("account_type"),
            row.get::<bool, _>("verified"),
            distance,
            radius,
            row.get::<bool, _>("active"),
            row.get::<String, _>("expected"),
            token
        );
    }

    print_count(
        pool,
        "posts",
        "SELECT count(*)::bigint FROM posts WHERE id = $1",
        post_id,
    )
    .await?;
    print_count(
        pool,
        "rescue_fanout_states",
        "SELECT count(*)::bigint FROM rescue_fanout_states WHERE post_id = $1",
        post_id,
    )
    .await?;
    print_count(
        pool,
        "rescue_fanout_attempts",
        "SELECT count(*)::bigint FROM rescue_fanout_attempts WHERE post_id = $1",
        post_id,
    )
    .await?;
    print_count(
        pool,
        "notification_events",
        "SELECT count(*)::bigint FROM notification_events WHERE post_id = $1",
        POST_ID,
    )
    .await?;
    print_count(pool, "push_delivery_jobs", "SELECT count(*)::bigint FROM push_delivery_jobs WHERE notification_event_id IN (SELECT id FROM notification_events WHERE post_id = $1)", POST_ID).await?;
    print_job_statuses(pool).await?;
    print_pipeline_timings(pool).await?;
    print_count(
        pool,
        "rescue_responses",
        "SELECT count(*)::bigint FROM rescue_responses WHERE post_id = $1",
        post_id,
    )
    .await?;
    print_count(
        pool,
        "chat_rooms_for_post",
        "SELECT count(*)::bigint FROM chat_rooms WHERE post_id = $1",
        post_id,
    )
    .await?;

    let jobs = sqlx::query(
        r#"
        SELECT
          j.id::text AS id,
          u.email::text AS email,
          j.status,
          j.attempts,
          j.last_error,
          j.provider_ticket_id,
          j.provider_accepted_at,
          j.receipt_status,
          j.receipt_checked_at,
          j.delivered_at,
          j.created_at
        FROM push_delivery_jobs j
        JOIN notification_events ne ON ne.id = j.notification_event_id
        LEFT JOIN users u ON u.id = j.user_id
        WHERE ne.post_id = $1
        ORDER BY j.created_at ASC
        "#,
    )
    .bind(POST_ID)
    .fetch_all(pool)
    .await?;

    println!("jobs:");
    for row in jobs {
        println!(
            "- id={} user={} status={} attempts={} ticket={:?} receipt={:?} accepted_at={:?} receipt_checked_at={:?} delivered_at={:?} last_error={:?}",
            row.get::<String, _>("id"),
            row.get::<Option<String>, _>("email").unwrap_or_else(|| "none".to_string()),
            row.get::<String, _>("status"),
            row.get::<i32, _>("attempts"),
            row.get::<Option<String>, _>("provider_ticket_id"),
            row.get::<Option<String>, _>("receipt_status"),
            row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("provider_accepted_at").map(|v| v.to_rfc3339()),
            row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("receipt_checked_at").map(|v| v.to_rfc3339()),
            row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("delivered_at").map(|v| v.to_rfc3339()),
            row.get::<Option<String>, _>("last_error")
        );
    }

    Ok(())
}

async fn simulate_response_and_chat(pool: &PgPool) -> anyhow::Result<()> {
    let post_id = Uuid::parse_str(POST_ID)?;
    let rescue_id = Uuid::parse_str(RESCUE_ID)?;
    let near_id = Uuid::parse_str(NEAR_100_ID)?;
    let author_id = Uuid::parse_str(AUTHOR_ID)?;

    sqlx::query(
        r#"
        INSERT INTO rescue_responses (
          rescue_session_id, post_id, user_id, action, status, lat, lng, eta_seconds
        )
        VALUES ($1, $2, $3, 'going', 'confirmed', $4, $5, 360)
        ON CONFLICT (post_id, user_id, action) DO UPDATE SET
          rescue_session_id = EXCLUDED.rescue_session_id,
          status = 'confirmed',
          lat = EXCLUDED.lat,
          lng = EXCLUDED.lng,
          eta_seconds = EXCLUDED.eta_seconds,
          updated_at = now()
        "#,
    )
    .bind(rescue_id)
    .bind(post_id)
    .bind(near_id)
    .bind(BASE_LAT + 0.0009)
    .bind(BASE_LNG)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        UPDATE rescue_fanout_states
        SET status = 'paused',
            confirmed_count = (
              SELECT count(*)::int
              FROM rescue_responses
              WHERE post_id = $1 AND action = 'going' AND status IN ('confirmed', 'arrived')
            ),
            arrived_count = (
              SELECT count(*)::int
              FROM rescue_responses
              WHERE post_id = $1 AND action = 'going' AND status = 'arrived'
            ),
            updated_at = now()
        WHERE post_id = $1
        "#,
    )
    .bind(post_id)
    .execute(pool)
    .await?;

    let room_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO chat_rooms (post_id, requester_id)
        VALUES ($1, $2)
        ON CONFLICT (post_id, requester_id)
          WHERE post_id IS NOT NULL AND requester_id IS NOT NULL
        DO UPDATE SET requester_id = EXCLUDED.requester_id
        RETURNING id
        "#,
    )
    .bind(post_id)
    .bind(near_id)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO chat_room_members (room_id, user_id)
        VALUES ($1, $2), ($1, $3)
        ON CONFLICT (room_id, user_id) DO NOTHING
        "#,
    )
    .bind(room_id)
    .bind(near_id)
    .bind(author_id)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO chat_messages (room_id, sender_id, body, idempotency_key)
        VALUES ($1, $2, 'Seed operacional: estou perto e posso ajudar.', $3)
        ON CONFLICT (room_id, sender_id, idempotency_key) DO NOTHING
        "#,
    )
    .bind(room_id)
    .bind(near_id)
    .bind(SEED_TAG)
    .execute(pool)
    .await?;

    println!("simulate_response_and_chat: ok room_id={room_id}");
    Ok(())
}

async fn print_job_statuses(pool: &PgPool) -> anyhow::Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT j.status, count(*)::bigint AS total
        FROM push_delivery_jobs j
        JOIN notification_events ne ON ne.id = j.notification_event_id
        WHERE ne.post_id = $1
        GROUP BY j.status
        ORDER BY j.status
        "#,
    )
    .bind(POST_ID)
    .fetch_all(pool)
    .await?;

    println!("job_statuses:");
    if rows.is_empty() {
        println!("- none");
    }
    for row in rows {
        println!(
            "- {}: {}",
            row.get::<String, _>("status"),
            row.get::<i64, _>("total")
        );
    }
    Ok(())
}

async fn print_pipeline_timings(pool: &PgPool) -> anyhow::Result<()> {
    let row = sqlx::query(
        r#"
        SELECT
          p.created_at AS post_created_at,
          min(j.created_at) AS first_job_created_at,
          min(j.provider_accepted_at) AS first_provider_accepted_at,
          min(j.delivered_at) AS first_delivered_at
        FROM posts p
        LEFT JOIN notification_events ne ON ne.post_id = p.id::text
        LEFT JOIN push_delivery_jobs j ON j.notification_event_id = ne.id
        WHERE p.id = $1
        GROUP BY p.created_at
        "#,
    )
    .bind(Uuid::parse_str(POST_ID)?)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        println!("pipeline_timings: post not found");
        return Ok(());
    };

    let post_created_at = row.get::<chrono::DateTime<chrono::Utc>, _>("post_created_at");
    let first_job_created_at =
        row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("first_job_created_at");
    let first_provider_accepted_at =
        row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("first_provider_accepted_at");
    let first_delivered_at =
        row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("first_delivered_at");

    println!("pipeline_timings:");
    println!("- post_created_at: {}", post_created_at.to_rfc3339());
    println!(
        "- post_to_first_job_ms: {}",
        delta_ms(post_created_at, first_job_created_at)
    );
    println!(
        "- post_to_provider_accepted_ms: {}",
        delta_ms(post_created_at, first_provider_accepted_at)
    );
    println!(
        "- post_to_receipt_delivered_ms: {}",
        delta_ms(post_created_at, first_delivered_at)
    );
    Ok(())
}

fn delta_ms(
    start: chrono::DateTime<chrono::Utc>,
    end: Option<chrono::DateTime<chrono::Utc>>,
) -> String {
    end.map(|value| (value - start).num_milliseconds().to_string())
        .unwrap_or_else(|| "pending".to_string())
}

async fn print_count<T>(
    pool: &PgPool,
    label: &str,
    sql: &'static str,
    bind: T,
) -> anyhow::Result<()>
where
    T: Send + Sync + 'static + sqlx::Encode<'static, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    let count: i64 = sqlx::query_scalar(sql).bind(bind).fetch_one(pool).await?;
    println!("{label}: {count}");
    Ok(())
}

fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("password hashing failed: {error}"))
}

fn seed_user_ids() -> anyhow::Result<Vec<Uuid>> {
    Ok(vec![
        Uuid::parse_str(AUTHOR_ID)?,
        Uuid::parse_str(NEAR_100_ID)?,
        Uuid::parse_str(NEAR_500_ID)?,
        Uuid::parse_str(NEAR_900_ID)?,
        Uuid::parse_str(FAR_5K_ID)?,
        Uuid::parse_str(NO_PUSH_ID)?,
        Uuid::parse_str(INVALID_PUSH_ID)?,
        Uuid::parse_str(ONG_USER_ID)?,
    ])
}

fn mask_token(token: &str) -> String {
    if token.len() <= 16 {
        return "***".to_string();
    }
    format!("{}...{}", &token[..12], &token[token.len() - 4..])
}
