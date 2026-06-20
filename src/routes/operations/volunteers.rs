use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{error::ApiError, routes::auth::authenticate_request, state::AppState};

const ALLOWED_ANIMAL_SCOPES: &[&str] = &[
    "dog",
    "cat",
    "bird",
    "wildlife",
    "reptile",
    "livestock",
    "marine",
    "general",
];

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct VolunteerProfileRequest {
    pub active: Option<bool>,
    #[validate(range(min = 0.3, max = 100.0))]
    pub service_radius_km: Option<f64>,
    #[serde(default)]
    pub animal_scopes: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[validate(length(max = 280))]
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolunteerProfileResponse {
    pub user_id: String,
    pub active: bool,
    pub service_radius_km: f64,
    pub animal_scopes: Vec<String>,
    pub capabilities: Vec<String>,
    pub notes: Option<String>,
    pub verified: bool,
    pub responses_count: i64,
    pub arrived_count: i64,
    pub updated_at: String,
}

pub async fn get_my_profile(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Option<VolunteerProfileResponse>>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;

    let row = sqlx::query(
        r#"
        SELECT user_id, active, service_radius_km, animal_scopes, capabilities, notes,
               verified, responses_count, arrived_count, updated_at
        FROM volunteer_profiles
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(row.map(row_to_response)))
}

pub async fn upsert_my_profile(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<VolunteerProfileRequest>,
) -> Result<Json<VolunteerProfileResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let animal_scopes = normalize_scopes(payload.animal_scopes)?;
    let capabilities = normalize_capabilities(payload.capabilities)?;
    let active = payload.active.unwrap_or(true);
    let radius = payload.service_radius_km.unwrap_or(8.0);

    let row = sqlx::query(
        r#"
        INSERT INTO volunteer_profiles (
          user_id, active, service_radius_km, animal_scopes, capabilities, notes
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (user_id)
        DO UPDATE SET
          active = EXCLUDED.active,
          service_radius_km = EXCLUDED.service_radius_km,
          animal_scopes = EXCLUDED.animal_scopes,
          capabilities = EXCLUDED.capabilities,
          notes = EXCLUDED.notes,
          updated_at = now()
        RETURNING user_id, active, service_radius_km, animal_scopes, capabilities, notes,
                  verified, responses_count, arrived_count, updated_at
        "#,
    )
    .bind(user_id)
    .bind(active)
    .bind(radius)
    .bind(&animal_scopes)
    .bind(&capabilities)
    .bind(payload.notes.as_deref().map(str::trim))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(row_to_response(row)))
}

fn normalize_scopes(values: Vec<String>) -> Result<Vec<String>, ApiError> {
    let mut scopes = if values.is_empty() {
        vec!["general".to_string()]
    } else {
        values
            .into_iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    };

    if scopes
        .iter()
        .any(|scope| !ALLOWED_ANIMAL_SCOPES.contains(&scope.as_str()))
    {
        return Err(ApiError::Validation(
            "animalScopes contains an unsupported value".into(),
        ));
    }

    scopes.sort();
    scopes.dedup();
    Ok(scopes)
}

fn normalize_capabilities(values: Vec<String>) -> Result<Vec<String>, ApiError> {
    let mut capabilities = Vec::new();
    for value in values {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if normalized.len() > 40 {
            return Err(ApiError::Validation(
                "capabilities values must be <= 40 chars".into(),
            ));
        }
        capabilities.push(normalized);
    }
    capabilities.sort();
    capabilities.dedup();
    Ok(capabilities)
}

fn row_to_response(row: sqlx::postgres::PgRow) -> VolunteerProfileResponse {
    VolunteerProfileResponse {
        user_id: row.get::<Uuid, _>("user_id").to_string(),
        active: row.get("active"),
        service_radius_km: row.get::<f64, _>("service_radius_km"),
        animal_scopes: row.get("animal_scopes"),
        capabilities: row.get("capabilities"),
        notes: row.get("notes"),
        verified: row.get("verified"),
        responses_count: row.get("responses_count"),
        arrived_count: row.get("arrived_count"),
        updated_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
            .to_rfc3339(),
    }
}
