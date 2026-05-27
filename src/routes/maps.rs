use std::time::Duration as StdDuration;

use axum::{extract::Query, extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    error::ApiError, routes::auth::authenticate_request, services::rate_limit, state::AppState,
};

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
    pub image_url: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GeocodeQuery {
    #[validate(length(min = 3, max = 240))]
    pub address: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PlaceAutocompleteQuery {
    #[validate(length(min = 3, max = 160))]
    pub input: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PlaceDetailsQuery {
    #[validate(length(min = 3, max = 240))]
    pub place_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceSuggestion {
    pub place_id: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceAutocompleteResponse {
    pub predictions: Vec<PlaceSuggestion>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeocodeResponse {
    pub label: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Deserialize)]
struct GoogleAutocompleteResponse {
    predictions: Option<Vec<GooglePrediction>>,
}

#[derive(Debug, Deserialize)]
struct GooglePrediction {
    place_id: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleGeocodeResponse {
    results: Option<Vec<GooglePlaceResult>>,
}

#[derive(Debug, Deserialize)]
struct GooglePlaceDetailsResponse {
    result: Option<GooglePlaceResult>,
}

#[derive(Debug, Deserialize)]
struct GooglePlaceResult {
    formatted_address: Option<String>,
    geometry: Option<GoogleGeometry>,
}

#[derive(Debug, Deserialize)]
struct GoogleGeometry {
    location: Option<GoogleLocation>,
}

#[derive(Debug, Deserialize)]
struct GoogleLocation {
    lat: f64,
    lng: f64,
}

pub async fn static_map_url(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<StaticMapQuery>,
) -> Result<Json<StaticMapResponse>, ApiError> {
    query
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    authorize_maps_request(&state, &headers, "static").await?;

    Ok(Json(StaticMapResponse {
        provider: "google".to_string(),
        image_url: None,
    }))
}

pub async fn geocode(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<GeocodeQuery>,
) -> Result<Json<Option<GeocodeResponse>>, ApiError> {
    query
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    authorize_maps_request(&state, &headers, "geocode").await?;
    Ok(Json(geocode_address(&state, &query.address).await?))
}

pub async fn geocode_address(
    state: &AppState,
    address: &str,
) -> Result<Option<GeocodeResponse>, ApiError> {
    let api_key = google_maps_key(state)?;
    let sanitized = sanitize_address(address);
    let url = format!(
        "https://maps.googleapis.com/maps/api/geocode/json?address={}&region=br&language=pt-BR&key={}",
        url_component(&sanitized),
        url_component(api_key),
    );
    let payload = reqwest::get(url)
        .await
        .map_err(|error| {
            tracing::warn!(?error, "google geocode request failed");
            ApiError::ServiceUnavailable
        })?
        .json::<GoogleGeocodeResponse>()
        .await
        .map_err(|error| {
            tracing::warn!(?error, "google geocode response parse failed");
            ApiError::ServiceUnavailable
        })?;

    Ok(payload
        .results
        .and_then(|mut results| results.drain(..).find_map(geocode_response_from_result)))
}

pub async fn place_autocomplete(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<PlaceAutocompleteQuery>,
) -> Result<Json<PlaceAutocompleteResponse>, ApiError> {
    query
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    authorize_maps_request(&state, &headers, "autocomplete").await?;
    let api_key = google_maps_key(&state)?;
    let sanitized = sanitize_address(&query.input);
    let url = format!(
        "https://maps.googleapis.com/maps/api/place/autocomplete/json?input={}&components=country%3Abr&types=address&language=pt-BR&key={}",
        url_component(&sanitized),
        url_component(api_key),
    );
    let payload = reqwest::get(url)
        .await
        .map_err(|error| {
            tracing::warn!(?error, "google places autocomplete request failed");
            ApiError::ServiceUnavailable
        })?
        .json::<GoogleAutocompleteResponse>()
        .await
        .map_err(|error| {
            tracing::warn!(?error, "google places autocomplete response parse failed");
            ApiError::ServiceUnavailable
        })?;

    let predictions = payload
        .predictions
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            Some(PlaceSuggestion {
                place_id: item.place_id?,
                description: item.description?,
            })
        })
        .take(5)
        .collect();

    Ok(Json(PlaceAutocompleteResponse { predictions }))
}

pub async fn place_details(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<PlaceDetailsQuery>,
) -> Result<Json<Option<GeocodeResponse>>, ApiError> {
    query
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    authorize_maps_request(&state, &headers, "details").await?;
    let api_key = google_maps_key(&state)?;
    let url = format!(
        "https://maps.googleapis.com/maps/api/place/details/json?place_id={}&fields=geometry,formatted_address&language=pt-BR&key={}",
        url_component(&query.place_id),
        url_component(api_key),
    );
    let payload = reqwest::get(url)
        .await
        .map_err(|error| {
            tracing::warn!(?error, "google place details request failed");
            ApiError::ServiceUnavailable
        })?
        .json::<GooglePlaceDetailsResponse>()
        .await
        .map_err(|error| {
            tracing::warn!(?error, "google place details response parse failed");
            ApiError::ServiceUnavailable
        })?;

    Ok(Json(payload.result.and_then(geocode_response_from_result)))
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

fn google_maps_key(state: &AppState) -> Result<&str, ApiError> {
    state
        .config
        .google_maps_api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .map(String::as_str)
        .ok_or_else(|| ApiError::Validation("GOOGLE_MAPS_API_KEY is required".into()))
}

fn geocode_response_from_result(result: GooglePlaceResult) -> Option<GeocodeResponse> {
    let location = result.geometry?.location?;
    Some(GeocodeResponse {
        label: result.formatted_address.unwrap_or_default(),
        latitude: location.lat,
        longitude: location.lng,
    })
}

fn sanitize_address(query: &str) -> String {
    query
        .trim()
        .replace("doutro", "Doutor")
        .replace("Doutro", "Doutor")
        .replace(" dr ", " Doutor ")
        .replace(" Dr ", " Doutor ")
}

async fn authorize_maps_request(
    state: &AppState,
    headers: &HeaderMap,
    action: &str,
) -> Result<(), ApiError> {
    let claims = authenticate_request(state, headers)?;
    rate_limit::check_key(
        state,
        &format!("maps:{action}:{}", claims.sub),
        state.config.throttle_limit * 3,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await
}
