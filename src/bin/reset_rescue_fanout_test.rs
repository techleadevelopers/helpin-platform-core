use anyhow::Context;
use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

const SEED_TAG: &str = "rescue_fanout_test";
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .context("failed to connect to database")?;

    reset_seed(&pool).await?;
    println!("reset_rescue_fanout_test: ok");
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

    Ok(())
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
