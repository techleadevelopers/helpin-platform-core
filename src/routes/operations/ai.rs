use std::time::{Duration as StdDuration, Instant};

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::AccountType, error::ApiError, routes::auth::authenticate_request, services::rate_limit,
    state::AppState,
};

const POST_ASSESSMENT_PROMPT_VERSION: &str = "post-assessment-v1";
const RESCUE_BRIEF_PROMPT_VERSION: &str = "rescue-brief-v1";

#[derive(Debug, Deserialize, Validate)]
pub struct ModerationJobRequest {
    #[validate(url)]
    pub image_url: String,
    pub post_id: String,
}

#[derive(Serialize)]
pub struct ModerationJobResponse {
    pub job_id: String,
    pub worker_url: String,
    pub status: &'static str,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PostAssessmentRequest {
    #[validate(length(min = 1, max = 4000))]
    pub description: String,
    #[validate(length(max = 180))]
    pub location: Option<String>,
    #[serde(default)]
    pub images: Vec<String>,
    pub declared_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostAssessmentResponse {
    pub suggested_type: String,
    pub urgency: String,
    pub risk_level: String,
    pub missing_info: Vec<String>,
    pub suggested_text: Option<String>,
    pub warnings: Vec<String>,
    pub generated_by_ai: bool,
    pub prompt_version: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueBriefResponse {
    pub summary: String,
    pub next_action: String,
    pub risk: String,
    pub checklist: Vec<String>,
    pub stale_alert: bool,
    pub generated_by_ai: bool,
    pub ai_model: Option<String>,
    pub prompt_version: String,
}

pub async fn enqueue_moderation_job(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<ModerationJobRequest>,
) -> Result<Json<ModerationJobResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    authenticate_request(&state, &headers)?;
    let job_id = uuid::Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO moderation_jobs (id, subject_type, subject_id, image_url, status, provider)
        VALUES ($1, 'post', $2, $3, 'queued', $4)
        "#,
    )
    .bind(job_id)
    .bind(&payload.post_id)
    .bind(&payload.image_url)
    .bind(&state.config.ai_worker_url)
    .execute(&state.db)
    .await?;

    Ok(Json(ModerationJobResponse {
        job_id: job_id.to_string(),
        worker_url: state.config.ai_worker_url,
        status: "queued",
    }))
}

pub async fn assess_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<PostAssessmentRequest>,
) -> Result<Json<PostAssessmentResponse>, ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    rate_limit::check_key(
        &state,
        &format!("ai:post-assessment:{user_id}"),
        state.config.throttle_limit * 3,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;

    let sanitized = PostAssessmentRequest {
        description: redact_sensitive_text(&payload.description),
        location: payload.location.as_deref().map(redact_sensitive_text),
        images: payload.images,
        declared_type: payload.declared_type,
    };

    let response = call_worker::<_, PostAssessmentResponse>(
        &state,
        "/ai/post-assessment",
        &sanitized,
        StdDuration::from_secs(3),
    )
    .await
    .unwrap_or_else(|error| {
        tracing::warn!(?error, user_id = %user_id, "post assessment worker failed; using fallback");
        post_assessment_fallback(&sanitized)
    });

    Ok(Json(response))
}

pub async fn rescue_brief(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RescueBriefResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let is_admin = matches!(claims.account_type, AccountType::Admin);
    rate_limit::check_key(
        &state,
        &format!("ai:rescue-brief:{id}:{user_id}"),
        state.config.throttle_limit * 3,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;

    if !can_manage_rescue(&state, id, user_id, is_admin).await? {
        return Err(ApiError::Forbidden);
    }

    let context = build_rescue_brief_context(&state, id).await?;
    let response = call_worker::<_, RescueBriefResponse>(
        &state,
        "/ai/rescue-brief",
        &context,
        StdDuration::from_secs(3),
    )
    .await
    .unwrap_or_else(|error| {
        tracing::warn!(?error, rescue_id = %id, "rescue brief worker failed; using fallback");
        rescue_brief_fallback(&context)
    });

    Ok(Json(response))
}

async fn call_worker<T, R>(
    state: &AppState,
    path: &str,
    payload: &T,
    timeout: StdDuration,
) -> Result<R, ApiError>
where
    T: Serialize + ?Sized,
    R: for<'de> Deserialize<'de>,
{
    let worker_url = state.config.ai_worker_url.trim().trim_end_matches('/');
    if worker_url.is_empty() {
        return Err(ApiError::ServiceUnavailable);
    }

    let started = Instant::now();
    let response = reqwest::Client::new()
        .post(format!("{worker_url}{path}"))
        .json(payload)
        .timeout(timeout)
        .send()
        .await
        .map_err(|_| ApiError::ServiceUnavailable)?;

    if !response.status().is_success() {
        return Err(ApiError::ServiceUnavailable);
    }

    let parsed = response
        .json::<R>()
        .await
        .map_err(|_| ApiError::ServiceUnavailable)?;
    tracing::info!(
        path,
        latency_ms = started.elapsed().as_millis() as u64,
        "ai worker response"
    );
    Ok(parsed)
}

async fn can_manage_rescue(
    state: &AppState,
    rescue_id: Uuid,
    user_id: Uuid,
    is_admin: bool,
) -> Result<bool, ApiError> {
    if is_admin {
        return Ok(true);
    }

    let owns = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
          SELECT 1
          FROM rescue_sessions rs
          INNER JOIN posts p ON p.id = rs.post_id
          WHERE rs.id = $1
            AND p.author_id = $2
        )
        "#,
    )
    .bind(rescue_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    Ok(owns)
}

async fn build_rescue_brief_context(
    state: &AppState,
    rescue_id: Uuid,
) -> Result<serde_json::Value, ApiError> {
    let rescue = sqlx::query(
        r#"
        SELECT rs.id::text AS id, rs.post_id, rs.status, rs.lat, rs.lng,
               rs.accuracy, rs.created_at, rs.updated_at,
               p.post_type::text AS post_type, p.animal_type, p.name,
               p.description, p.neighborhood, p.location_label, p.rescue_status
        FROM rescue_sessions rs
        INNER JOIN posts p ON p.id = rs.post_id
        WHERE rs.id = $1
        "#,
    )
    .bind(rescue_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    let incidents = sqlx::query(
        r#"
        SELECT description, status, created_at
        FROM rescue_incidents
        WHERE rescue_id = $1
        ORDER BY created_at ASC
        LIMIT 20
        "#,
    )
    .bind(rescue_id)
    .fetch_all(&state.db)
    .await?;

    let responses = sqlx::query(
        r#"
        SELECT action, status, eta_seconds, created_at, updated_at
        FROM rescue_responses
        WHERE rescue_session_id = $1 OR post_id = $2
        ORDER BY updated_at ASC
        LIMIT 40
        "#,
    )
    .bind(rescue_id)
    .bind(rescue.get::<Uuid, _>("post_id"))
    .fetch_all(&state.db)
    .await?;

    let volunteers_going = responses
        .iter()
        .filter(|row| row.get::<String, _>("action") == "going")
        .filter(|row| {
            matches!(
                row.get::<String, _>("status").as_str(),
                "confirmed" | "arrived"
            )
        })
        .count();

    Ok(json!({
        "rescue_id": rescue_id.to_string(),
        "post": {
            "id": rescue.get::<Uuid, _>("post_id").to_string(),
            "type": rescue.get::<String, _>("post_type"),
            "animalType": rescue.get::<String, _>("animal_type"),
            "name": rescue.get::<Option<String>, _>("name").map(|value| redact_sensitive_text(&value)),
            "description": redact_sensitive_text(&rescue.get::<String, _>("description")),
            "neighborhood": rescue.get::<Option<String>, _>("neighborhood"),
            "locationLabel": rescue.get::<Option<String>, _>("location_label"),
            "rescueStatus": rescue.get::<String, _>("rescue_status"),
        },
        "location": {
            "lat": rescue.get::<f64, _>("lat"),
            "lng": rescue.get::<f64, _>("lng"),
            "accuracy": rescue.get::<Option<f64>, _>("accuracy"),
            "neighborhood": rescue.get::<Option<String>, _>("neighborhood"),
        },
        "volunteers_going": volunteers_going,
        "chat_messages": [],
        "incidents": incidents.into_iter().map(|row| json!({
            "description": redact_sensitive_text(&row.get::<String, _>("description")),
            "status": row.get::<String, _>("status"),
            "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
        })).collect::<Vec<_>>(),
        "last_update_at": rescue.get::<chrono::DateTime<chrono::Utc>, _>("updated_at").to_rfc3339(),
    }))
}

fn post_assessment_fallback(payload: &PostAssessmentRequest) -> PostAssessmentResponse {
    let text = payload.description.to_lowercase();
    let mut missing_info = Vec::new();
    if payload
        .location
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        missing_info.push("localizacao".to_string());
    }
    if !["ferido", "machucado", "atropelado", "preso"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        missing_info.push("estado do animal".to_string());
    }
    let urgency = if ["atropelado", "sangue", "ferido", "preso"]
        .iter()
        .any(|marker| text.contains(marker))
    {
        "high"
    } else {
        "medium"
    };
    let suggested_type = payload.declared_type.clone().unwrap_or_else(|| {
        if urgency == "high" {
            "emergency"
        } else {
            "post"
        }
        .to_string()
    });

    PostAssessmentResponse {
        suggested_type,
        urgency: urgency.to_string(),
        risk_level: if missing_info.is_empty() {
            "low"
        } else {
            "medium"
        }
        .to_string(),
        missing_info,
        suggested_text: None,
        warnings: vec![
            "Nao informe telefone, endereco completo ou dados pessoais no texto publico."
                .to_string(),
        ],
        generated_by_ai: false,
        prompt_version: POST_ASSESSMENT_PROMPT_VERSION.to_string(),
    }
}

fn rescue_brief_fallback(context: &serde_json::Value) -> RescueBriefResponse {
    let volunteers_going = context
        .get("volunteers_going")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let incident_count = context
        .get("incidents")
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0);
    let risk = if incident_count > 0 {
        "high"
    } else if volunteers_going == 0 {
        "medium"
    } else {
        "low"
    };

    RescueBriefResponse {
        summary: format!(
            "Resgate em acompanhamento com {volunteers_going} voluntario(s) confirmado(s)."
        ),
        next_action: if volunteers_going > 0 {
            "Confirmar chegada de um voluntario e registrar foto do local.".to_string()
        } else {
            "Acionar voluntario ou ONG proxima e confirmar localizacao.".to_string()
        },
        risk: risk.to_string(),
        checklist: vec![
            "Confirmar localizacao antes do deslocamento".to_string(),
            "Registrar atualizacao ao chegar".to_string(),
            "Evitar prometer atendimento antes da confirmacao humana".to_string(),
        ],
        stale_alert: false,
        generated_by_ai: false,
        ai_model: None,
        prompt_version: RESCUE_BRIEF_PROMPT_VERSION.to_string(),
    }
}

fn redact_sensitive_text(value: &str) -> String {
    value
        .split_whitespace()
        .map(|token| {
            let digits = token.chars().filter(|char| char.is_ascii_digit()).count();
            if token.contains('@') && token.contains('.') {
                "[email]".to_string()
            } else if digits >= 8 {
                "[telefone]".to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
