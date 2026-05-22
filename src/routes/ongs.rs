use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    domain::{seed_ongs, Ong},
    error::ApiError,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct OngQuery {
    pub q: Option<String>,
    pub city: Option<String>,
    pub verified: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowOngResponse {
    pub ong_id: String,
    pub following: bool,
}

pub async fn list_ongs(
    State(state): State<AppState>,
    Query(query): Query<OngQuery>,
) -> Json<Vec<Ong>> {
    let db_ongs = load_db_ongs(&state).await.unwrap_or_else(|error| {
        tracing::warn!(
            ?error,
            "database ONG list unavailable; using seed ONGs only"
        );
        Vec::new()
    });

    Json(filter_ongs(query, db_ongs))
}

pub async fn get_ong(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Ong>, ApiError> {
    if let Ok(uuid) = Uuid::parse_str(&id) {
        if let Some(ong) = load_db_ong(&state, uuid).await? {
            return Ok(Json(ong));
        }
    }

    seed_ongs()
        .into_iter()
        .find(|ong| ong.id == id && ong.verified)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn follow_ong(Path(id): Path<String>) -> Result<Json<FollowOngResponse>, ApiError> {
    if seed_ongs().iter().any(|ong| ong.id == id) || Uuid::parse_str(&id).is_ok() {
        return Ok(Json(FollowOngResponse {
            ong_id: id,
            following: true,
        }));
    }

    Err(ApiError::NotFound)
}

fn filter_ongs(query: OngQuery, db_ongs: Vec<Ong>) -> Vec<Ong> {
    let q = query.q.map(|value| value.to_lowercase());
    let city = query.city.map(|value| value.to_lowercase());
    let verified_filter = query.verified.unwrap_or(true);

    let mut ongs = db_ongs;
    ongs.extend(seed_ongs());

    ongs.into_iter()
        .filter(|ong| ong.verified == verified_filter)
        .filter(|ong| {
            q.as_ref().map_or(true, |q| {
                ong.name.to_lowercase().contains(q)
                    || ong.cause.to_lowercase().contains(q)
                    || ong.city.to_lowercase().contains(q)
            })
        })
        .filter(|ong| {
            city.as_ref()
                .map_or(true, |city| ong.city.to_lowercase() == *city)
        })
        .collect()
}

async fn load_db_ongs(state: &AppState) -> Result<Vec<Ong>, sqlx::Error> {
    let sql = db_ong_select_sql("");
    let rows = sqlx::query(&sql).fetch_all(&state.db).await?;

    Ok(rows.into_iter().map(row_to_ong).collect())
}

async fn load_db_ong(state: &AppState, id: Uuid) -> Result<Option<Ong>, sqlx::Error> {
    let sql = db_ong_select_sql("WHERE op.id = $1 AND op.verification_status = 'APPROVED'");
    let row = sqlx::query(&sql).bind(id).fetch_optional(&state.db).await?;

    Ok(row.map(row_to_ong))
}

fn db_ong_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT
          op.id,
          op.legal_name,
          op.mission,
          op.city,
          op.state,
          op.cnpj,
          op.contact_phone,
          op.area_type,
          op.neighborhood,
          op.verification_status,
          op.created_at,
          COALESCE(active_cases.count, 0)::int AS active_cases
        FROM ong_profiles op
        LEFT JOIN LATERAL (
          SELECT COUNT(*) AS count
          FROM posts p
          WHERE p.author_id = op.user_id
            AND p.moderation_status = 'approved'
        ) active_cases ON true
        {where_clause}
        ORDER BY op.created_at DESC
        LIMIT 500
        "#
    )
}

fn row_to_ong(row: sqlx::postgres::PgRow) -> Ong {
    let name: String = row.get("legal_name");
    let city: Option<String> = row.get("city");
    let state: Option<String> = row.get("state");
    let neighborhood: Option<String> = row.get("neighborhood");
    let verification_status: String = row.get("verification_status");
    let created_at: DateTime<Utc> = row.get("created_at");
    let short_name = initials(&name);
    let city_label = city.clone().unwrap_or_else(|| "Brasil".into());
    let state_label = state.clone().unwrap_or_default();
    let location = [
        neighborhood,
        Some(city_label.clone()),
        (!state_label.is_empty()).then_some(state_label.clone()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");

    Ong {
        id: row.get::<Uuid, _>("id").to_string(),
        name: name.clone(),
        short_name,
        description: row
            .get::<Option<String>, _>("mission")
            .unwrap_or_else(|| "ONG verificada na rede ZooHelp.".into()),
        mission: row
            .get::<Option<String>, _>("mission")
            .unwrap_or_else(|| "Protecao animal e apoio a comunidade.".into()),
        location,
        city: city_label,
        state: state_label,
        verified: verification_status == "APPROVED",
        animals_rescued: 0,
        active_cases: row.get::<i32, _>("active_cases").max(0) as u32,
        adoptions: 0,
        animal_types: vec!["Todos".into()],
        followers: 0,
        since: created_at.year().to_string(),
        cnpj: row.get::<Option<String>, _>("cnpj").unwrap_or_default(),
        contact: row
            .get::<Option<String>, _>("contact_phone")
            .unwrap_or_default(),
        cause: row
            .get::<Option<String>, _>("area_type")
            .unwrap_or_else(|| "Protecao animal".into()),
    }
}

fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(3)
        .collect();

    if letters.is_empty() {
        "ONG".into()
    } else {
        letters.to_uppercase()
    }
}
