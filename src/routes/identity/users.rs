use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    domain::{AccountType, Post},
    error::ApiError,
    routes::{
        auth::authenticate_request,
        feed::{load_db_posts, FeedQuery},
        posts::optional_authenticated_user_id,
    },
    services::auth as auth_service,
    state::AppState,
};

const PUBLIC_PROFILE_POST_LIMIT: usize = 60;

#[derive(Deserialize)]
pub struct PublicUserSearchQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUserSummary {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub verified: bool,
}

#[derive(Deserialize)]
pub struct PublicUserRelationQuery {
    pub limit: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUserProfile {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub verified: bool,
    pub bio: String,
    pub location: String,
    pub posts_count: i64,
    pub active_cases_count: i64,
    pub resolved_cases_count: i64,
    pub followers_count: i64,
    pub following_count: i64,
    pub following: bool,
    pub posts: Vec<Post>,
    pub created_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUserResponse {
    pub user_id: String,
    pub following: bool,
    pub followers_count: i64,
}

pub async fn list_public_users(
    State(state): State<AppState>,
    Query(query): Query<PublicUserSearchQuery>,
) -> Result<Json<Vec<PublicUserSummary>>, ApiError> {
    let term = query.q.unwrap_or_default().trim().to_lowercase();
    let limit = query.limit.unwrap_or(20).clamp(1, 50);

    let rows = sqlx::query(
        r#"
        SELECT
          u.id,
          u.name,
          u.avatar_url,
          u.account_type::text AS account_type,
          u.verified,
          op.verification_status
        FROM users u
        LEFT JOIN ong_profiles op ON op.user_id = u.id
        WHERE u.deleted_at IS NULL
          AND (
            $1 = ''
            OR lower(u.name) LIKE '%' || $1 || '%'
            OR lower(COALESCE(u.city, '')) LIKE '%' || $1 || '%'
            OR lower(COALESCE(u.neighborhood, '')) LIKE '%' || $1 || '%'
          )
        ORDER BY
          CASE WHEN $1 = '' THEN u.created_at END DESC,
          lower(u.name) ASC
        LIMIT $2
        "#,
    )
    .bind(term)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    let users = rows.into_iter().map(public_user_summary_from_row).collect();

    Ok(Json(users))
}

pub async fn get_public_user_profile(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PublicUserProfile>, ApiError> {
    let user_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let viewer_id = optional_authenticated_user_id(&state, &headers);
    load_public_profile(&state, user_id, viewer_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn list_public_user_followers(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PublicUserRelationQuery>,
) -> Result<Json<Vec<PublicUserSummary>>, ApiError> {
    let user_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let users = list_user_relation_summaries(
        &state,
        r#"
        SELECT
          u.id,
          u.name,
          u.avatar_url,
          u.account_type::text AS account_type,
          u.verified,
          op.verification_status
        FROM user_follows uf
        INNER JOIN users u ON u.id = uf.follower_id
        LEFT JOIN ong_profiles op ON op.user_id = u.id
        WHERE uf.followed_id = $1
          AND uf.active = true
          AND u.deleted_at IS NULL
        ORDER BY uf.updated_at DESC
        LIMIT $2
        "#,
        user_id,
        limit,
    )
    .await?;
    Ok(Json(users))
}

pub async fn list_public_user_following(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PublicUserRelationQuery>,
) -> Result<Json<Vec<PublicUserSummary>>, ApiError> {
    let user_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let users = list_user_relation_summaries(
        &state,
        r#"
        SELECT
          u.id,
          u.name,
          u.avatar_url,
          u.account_type::text AS account_type,
          u.verified,
          op.verification_status
        FROM user_follows uf
        INNER JOIN users u ON u.id = uf.followed_id
        LEFT JOIN ong_profiles op ON op.user_id = u.id
        WHERE uf.follower_id = $1
          AND uf.active = true
          AND u.deleted_at IS NULL
        ORDER BY uf.updated_at DESC
        LIMIT $2
        "#,
        user_id,
        limit,
    )
    .await?;
    Ok(Json(users))
}

pub async fn follow_user(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<FollowUserResponse>, ApiError> {
    set_follow_user(headers, state, id, true).await
}

pub async fn unfollow_user(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<FollowUserResponse>, ApiError> {
    set_follow_user(headers, state, id, false).await
}

async fn set_follow_user(
    headers: HeaderMap,
    state: AppState,
    id: String,
    should_follow: bool,
) -> Result<Json<FollowUserResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let follower_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let followed_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    if follower_id == followed_id {
        return Err(ApiError::Validation("cannot follow yourself".into()));
    }

    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id = $1 AND deleted_at IS NULL")
            .bind(followed_id)
            .fetch_optional(&state.db)
            .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }

    if should_follow {
        sqlx::query(
            r#"
            INSERT INTO user_follows (follower_id, followed_id, active)
            VALUES ($1, $2, true)
            ON CONFLICT (follower_id, followed_id)
            DO UPDATE SET active = true, updated_at = now()
            "#,
        )
        .bind(follower_id)
        .bind(followed_id)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query(
            r#"
            INSERT INTO user_follows (follower_id, followed_id, active)
            VALUES ($1, $2, false)
            ON CONFLICT (follower_id, followed_id)
            DO UPDATE SET active = false, updated_at = now()
            "#,
        )
        .bind(follower_id)
        .bind(followed_id)
        .execute(&state.db)
        .await?;
    }

    let following: bool = sqlx::query_scalar(
        "SELECT active FROM user_follows WHERE follower_id = $1 AND followed_id = $2",
    )
    .bind(follower_id)
    .bind(followed_id)
    .fetch_one(&state.db)
    .await?;
    let followers_count = followers_count(&state, followed_id).await?;

    Ok(Json(FollowUserResponse {
        user_id: id,
        following,
        followers_count,
    }))
}

async fn list_user_relation_summaries(
    state: &AppState,
    sql: &str,
    user_id: Uuid,
    limit: i64,
) -> Result<Vec<PublicUserSummary>, ApiError> {
    let rows = sqlx::query(sql)
        .bind(user_id)
        .bind(limit)
        .fetch_all(&state.db)
        .await?;

    Ok(rows.into_iter().map(public_user_summary_from_row).collect())
}

fn public_user_summary_from_row(row: sqlx::postgres::PgRow) -> PublicUserSummary {
    let account_type = auth_service::account_type_from_str(row.get::<&str, _>("account_type"));
    let verified = if matches!(account_type, AccountType::Ong) {
        row.get::<Option<String>, _>("verification_status")
            .as_deref()
            == Some("APPROVED")
    } else {
        row.get("verified")
    };
    PublicUserSummary {
        id: row.get::<Uuid, _>("id").to_string(),
        name: row.get("name"),
        avatar: row.get("avatar_url"),
        account_type,
        verified,
    }
}

async fn load_public_profile(
    state: &AppState,
    user_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Option<PublicUserProfile>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
          u.id,
          u.name,
          u.avatar_url,
          u.account_type::text AS account_type,
          u.verified,
          u.created_at,
          u.neighborhood,
          u.city,
          u.state,
          op.verification_status,
          op.mission,
          op.neighborhood AS ong_neighborhood,
          op.city AS ong_city,
          op.state AS ong_state,
          COALESCE(posts.count, 0)::bigint AS posts_count,
          COALESCE(active.count, 0)::bigint AS active_cases_count,
          COALESCE(resolved.count, 0)::bigint AS resolved_cases_count,
          COALESCE(followers.count, 0)::bigint AS followers_count,
          COALESCE(following.count, 0)::bigint AS following_count,
          EXISTS(
            SELECT 1 FROM user_follows uf
            WHERE uf.follower_id = $2 AND uf.followed_id = u.id AND uf.active = true
          ) AS following
        FROM users u
        LEFT JOIN ong_profiles op ON op.user_id = u.id
        LEFT JOIN LATERAL (
          SELECT count(*) FROM posts p
          WHERE p.author_id = u.id AND p.moderation_status = 'approved'
        ) posts ON true
        LEFT JOIN LATERAL (
          SELECT count(*) FROM posts p
          WHERE p.author_id = u.id AND p.moderation_status = 'approved'
            AND p.rescue_status <> 'resolved'
            AND (p.urgent = true OR p.post_type IN ('emergency', 'lost', 'found'))
        ) active ON true
        LEFT JOIN LATERAL (
          SELECT count(*) FROM posts p
          WHERE p.author_id = u.id AND p.moderation_status = 'approved'
            AND p.rescue_status = 'resolved'
        ) resolved ON true
        LEFT JOIN LATERAL (
          SELECT count(*) FROM user_follows uf
          WHERE uf.followed_id = u.id AND uf.active = true
        ) followers ON true
        LEFT JOIN LATERAL (
          SELECT count(*) FROM user_follows uf
          WHERE uf.follower_id = u.id AND uf.active = true
        ) following ON true
        WHERE u.id = $1 AND u.deleted_at IS NULL
        "#,
    )
    .bind(user_id)
    .bind(viewer_id)
    .fetch_optional(&state.db)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let account_type = auth_service::account_type_from_str(row.get::<&str, _>("account_type"));
    let verified = if matches!(account_type, AccountType::Ong) {
        row.get::<Option<String>, _>("verification_status")
            .as_deref()
            == Some("APPROVED")
    } else {
        row.get("verified")
    };

    let posts = load_profile_posts(state, user_id, viewer_id).await?;
    Ok(Some(PublicUserProfile {
        id: row.get::<Uuid, _>("id").to_string(),
        name: row.get("name"),
        avatar: row.get("avatar_url"),
        account_type: account_type.clone(),
        verified,
        bio: public_bio(&account_type, row.get::<Option<String>, _>("mission")),
        location: public_location(
            row.get::<Option<String>, _>("neighborhood"),
            row.get::<Option<String>, _>("city"),
            row.get::<Option<String>, _>("state"),
            row.get::<Option<String>, _>("ong_neighborhood"),
            row.get::<Option<String>, _>("ong_city"),
            row.get::<Option<String>, _>("ong_state"),
            &account_type,
        ),
        posts_count: row.get("posts_count"),
        active_cases_count: row.get("active_cases_count"),
        resolved_cases_count: row.get("resolved_cases_count"),
        followers_count: row.get("followers_count"),
        following_count: row.get("following_count"),
        following: row.get("following"),
        posts,
        created_at: row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
    }))
}

async fn load_profile_posts(
    state: &AppState,
    user_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Vec<Post>, ApiError> {
    let mut posts = load_db_posts(
        state,
        &FeedQuery {
            post_type: None,
            author_type: None,
            urgent: None,
            lat: None,
            lng: None,
            radius_km: None,
            limit: Some(PUBLIC_PROFILE_POST_LIMIT),
            before: None,
            liked: None,
            author_id: Some(user_id),
        },
        viewer_id,
    )
    .await?;
    posts.truncate(PUBLIC_PROFILE_POST_LIMIT);
    Ok(posts)
}

async fn followers_count(state: &AppState, followed_id: Uuid) -> Result<i64, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM user_follows WHERE followed_id = $1 AND active = true",
    )
    .bind(followed_id)
    .fetch_one(&state.db)
    .await?)
}

fn public_bio(account_type: &AccountType, mission: Option<String>) -> String {
    if matches!(account_type, AccountType::Ong) {
        return mission
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "ONG verificada na rede Helpin.".into());
    }
    if matches!(account_type, AccountType::Vet) {
        return "Perfil veterinario na rede Helpin.".into();
    }
    "Perfil da comunidade Helpin.".into()
}

fn public_location(
    user_neighborhood: Option<String>,
    user_city: Option<String>,
    user_state: Option<String>,
    ong_neighborhood: Option<String>,
    ong_city: Option<String>,
    ong_state: Option<String>,
    account_type: &AccountType,
) -> String {
    let (neighborhood, city, state) = if matches!(account_type, AccountType::Ong) {
        (ong_neighborhood, ong_city, ong_state)
    } else {
        (user_neighborhood, user_city, user_state)
    };
    [neighborhood, city, state.map(|value| value.to_uppercase())]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}
