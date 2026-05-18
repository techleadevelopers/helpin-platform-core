use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct OngProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub verified: bool,
    pub trust_score: u8,
}

pub async fn list_ongs() -> Json<Vec<OngProfile>> {
    Json(vec![OngProfile {
        id: "ong-dev",
        name: "ONG verificada",
        verified: true,
        trust_score: 92,
    }])
}
