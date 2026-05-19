use std::{collections::HashMap, sync::LazyLock};

use axum::{extract::Path, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use validator::Validate;

use crate::error::ApiError;

static RESCUES: LazyLock<Mutex<HashMap<String, RescueSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueSession {
    pub id: String,
    pub post_id: String,
    pub status: String,
    pub lat: f64,
    pub lng: f64,
    pub accuracy: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TriggerRescueRequest {
    #[validate(length(min = 1, max = 120))]
    pub post_id: String,
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    pub accuracy: Option<f64>,
}

#[derive(Deserialize, Validate)]
pub struct LocationUpdateRequest {
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    pub accuracy: Option<f64>,
}

#[derive(Deserialize, Validate)]
pub struct IncidentRequest {
    #[validate(length(min = 1, max = 2000))]
    pub description: String,
    #[serde(default)]
    pub attachments: Vec<String>,
}

#[derive(Serialize)]
pub struct RescueResponse {
    pub rescue: RescueSession,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentResponse {
    pub id: String,
    pub rescue_id: String,
    pub status: &'static str,
}

pub async fn trigger(
    Json(payload): Json<TriggerRescueRequest>,
) -> Result<(StatusCode, Json<RescueResponse>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let now = chrono::Utc::now().to_rfc3339();
    let rescue = RescueSession {
        id: uuid::Uuid::now_v7().to_string(),
        post_id: payload.post_id,
        status: "active".to_string(),
        lat: payload.lat,
        lng: payload.lng,
        accuracy: payload.accuracy,
        created_at: now.clone(),
        updated_at: now,
    };
    RESCUES
        .lock()
        .map_err(|_| ApiError::Internal)?
        .insert(rescue.id.clone(), rescue.clone());
    Ok((StatusCode::CREATED, Json(RescueResponse { rescue })))
}

pub async fn update_location(
    Path(id): Path<String>,
    Json(payload): Json<LocationUpdateRequest>,
) -> Result<Json<RescueResponse>, ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let mut rescues = RESCUES.lock().map_err(|_| ApiError::Internal)?;
    let rescue = rescues.get_mut(&id).ok_or(ApiError::NotFound)?;
    rescue.lat = payload.lat;
    rescue.lng = payload.lng;
    rescue.accuracy = payload.accuracy;
    rescue.updated_at = chrono::Utc::now().to_rfc3339();
    Ok(Json(RescueResponse {
        rescue: rescue.clone(),
    }))
}

pub async fn end(Path(id): Path<String>) -> Result<Json<RescueResponse>, ApiError> {
    let mut rescues = RESCUES.lock().map_err(|_| ApiError::Internal)?;
    let rescue = rescues.get_mut(&id).ok_or(ApiError::NotFound)?;
    rescue.status = "ended".to_string();
    rescue.updated_at = chrono::Utc::now().to_rfc3339();
    Ok(Json(RescueResponse {
        rescue: rescue.clone(),
    }))
}

pub async fn incident(
    Path(id): Path<String>,
    Json(payload): Json<IncidentRequest>,
) -> Result<(StatusCode, Json<IncidentResponse>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let _attachments = payload.attachments;
    if !RESCUES
        .lock()
        .map_err(|_| ApiError::Internal)?
        .contains_key(&id)
    {
        return Err(ApiError::NotFound);
    }
    Ok((
        StatusCode::CREATED,
        Json(IncidentResponse {
            id: uuid::Uuid::now_v7().to_string(),
            rescue_id: id,
            status: "queued_review",
        }),
    ))
}
