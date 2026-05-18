use axum::{extract::Path, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    domain::{seed_authors, seed_posts, AnimalType, Post, PostType},
    error::ApiError,
    services::fraud,
};

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreatePostRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: Option<String>,
    pub post_type: PostType,
    pub animal_type: AnimalType,
    pub breed: Option<String>,
    pub age: Option<String>,
    #[validate(length(min = 1, max = 1200))]
    pub description: String,
    #[validate(length(min = 1, max = 180))]
    pub location: String,
    pub neighborhood: Option<String>,
    pub image: Option<String>,
    pub urgent: Option<bool>,
    pub contact: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePostResponse {
    pub post: Post,
    pub moderation_status: &'static str,
    pub fraud_risk: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeResponse {
    pub post_id: String,
    pub liked: bool,
}

pub async fn create_post(
    Json(payload): Json<CreatePostRequest>,
) -> Result<(StatusCode, Json<CreatePostResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let risk = fraud::score_post_text(&payload.description);
    let author = seed_authors()
        .into_iter()
        .find(|author| author.id == "u5")
        .ok_or(ApiError::Internal)?;

    let post = Post {
        id: uuid::Uuid::now_v7().to_string(),
        post_type: payload.post_type,
        animal_type: payload.animal_type,
        name: payload.name.unwrap_or_else(|| "Publicação".into()),
        breed: payload.breed.unwrap_or_default(),
        age: payload.age.unwrap_or_default(),
        description: payload.description,
        location: payload.location.clone(),
        neighborhood: payload.neighborhood.unwrap_or(payload.location),
        image: payload.image,
        text_only: false,
        author,
        likes: 0,
        comments: 0,
        shares: 0,
        urgent: payload.urgent.unwrap_or(false),
        created_at: "agora".into(),
        contact: payload.contact.unwrap_or_default(),
        tags: payload.tags.unwrap_or_default(),
        latitude: -23.5505,
        longitude: -46.6333,
    };

    Ok((
        StatusCode::CREATED,
        Json(CreatePostResponse {
            post,
            moderation_status: "queued",
            fraud_risk: risk,
        }),
    ))
}

pub async fn get_post(Path(id): Path<String>) -> Result<Json<Post>, ApiError> {
    seed_posts()
        .into_iter()
        .find(|post| post.id == id)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn toggle_like(Path(id): Path<String>) -> Result<Json<LikeResponse>, ApiError> {
    if seed_posts().iter().any(|post| post.id == id) {
        return Ok(Json(LikeResponse {
            post_id: id,
            liked: true,
        }));
    }

    Err(ApiError::NotFound)
}
