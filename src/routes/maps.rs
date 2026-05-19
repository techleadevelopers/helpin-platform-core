use axum::{extract::Query, extract::State, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{error::ApiError, state::AppState};

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct StaticMapQuery {
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    #[validate(range(min = 1, max = 20))]
    pub zoom: Option<u8>,
    #[validate(range(min = 160, max = 1280))]
    pub width: Option<u16>,
    #[validate(range(min = 120, max = 1280))]
    pub height: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticMapResponse {
    pub provider: String,
    pub image_url: String,
}

pub async fn static_map_url(
    State(state): State<AppState>,
    Query(query): Query<StaticMapQuery>,
) -> Result<Json<StaticMapResponse>, ApiError> {
    query
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    let provider = state
        .config
        .geocoding_api_provider
        .clone()
        .unwrap_or_else(|| "google".to_string())
        .to_lowercase();

    if provider != "google" && provider != "google_maps" {
        return Err(ApiError::Validation(format!(
            "unsupported geocoding provider: {provider}"
        )));
    }

    let api_key = state
        .config
        .google_maps_api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::Validation("GOOGLE_MAPS_API_KEY is required".into()))?;

    let zoom = query.zoom.unwrap_or(14);
    let width = query.width.unwrap_or(640);
    let height = query.height.unwrap_or(320);
    let marker = format!("color:green|label:Z|{},{}", query.lat, query.lng);
    let image_url = format!(
        "https://maps.googleapis.com/maps/api/staticmap?center={},{}&zoom={zoom}&size={}x{}&scale=2&maptype=roadmap&markers={}&key={}",
        query.lat,
        query.lng,
        width,
        height,
        url_component(&marker),
        url_component(api_key),
    );

    Ok(Json(StaticMapResponse {
        provider: "google".to_string(),
        image_url,
    }))
}

fn url_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}
