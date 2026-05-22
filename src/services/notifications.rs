#[cfg(test)]
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{domain::Post, services::geo::haversine_km};

#[cfg(test)]
const MAX_RECENT_ALERTS: usize = 200;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PushPlatform {
    Ios,
    Android,
    Expo,
    Web,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscription {
    pub user_id: String,
    pub push_token: String,
    pub platform: PushPlatform,
    pub lat: f64,
    pub lng: f64,
    pub radius_km: f64,
    pub critical_alerts: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueAlert {
    pub id: String,
    pub post_id: String,
    pub title: String,
    pub body: String,
    pub image_url: Option<String>,
    pub lat: f64,
    pub lng: f64,
    pub radius_km: f64,
    pub critical: bool,
    pub actions: Vec<NotificationAction>,
    pub recipients: Vec<AlertRecipient>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRecipient {
    pub user_id: String,
    pub push_token: String,
    pub platform: PushPlatform,
    pub distance_km: f64,
    pub delivery_status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationAction {
    pub id: &'static str,
    pub label: &'static str,
    pub deep_link: String,
}

#[cfg(test)]
#[derive(Clone, Default)]
pub struct NotificationEngine {
    subscriptions: Arc<Mutex<HashMap<String, PushSubscription>>>,
    #[cfg(test)]
    recent_alerts: Arc<Mutex<VecDeque<RescueAlert>>>,
}

#[cfg(test)]
impl NotificationEngine {
    pub fn upsert_subscription(&self, subscription: PushSubscription) -> usize {
        let mut subscriptions = self.subscriptions.lock().expect("notification lock");
        subscriptions.insert(subscription.push_token.clone(), subscription);
        subscriptions.len()
    }

    #[cfg(test)]
    pub fn dispatch_rescue_alert(&self, post: &Post, default_radius_km: f64) -> RescueAlert {
        let radius_km = if post.urgent { 8.0 } else { default_radius_km }.clamp(1.0, 50.0);
        let recipients = self.nearby_recipients(post.latitude, post.longitude, radius_km);
        let title = if post.urgent {
            "Resgate urgente perto de voce".to_string()
        } else {
            "Animal precisa de ajuda perto de voce".to_string()
        };
        let body = format!(
            "{} em {}. Toque para ir ao local ou apoiar pelo chat.",
            post.name, post.neighborhood
        );
        let alert = RescueAlert {
            id: uuid::Uuid::now_v7().to_string(),
            post_id: post.id.clone(),
            title,
            body,
            image_url: post.image.clone(),
            lat: post.latitude,
            lng: post.longitude,
            radius_km,
            critical: post.urgent,
            actions: vec![
                NotificationAction {
                    id: "go_to_location",
                    label: "Ir ao local",
                    deep_link: format!("zoohelp://post/{}?action=route", post.id),
                },
                NotificationAction {
                    id: "remote_support",
                    label: "Apoiar remoto",
                    deep_link: format!("zoohelp://post/{}?action=chat", post.id),
                },
            ],
            recipients,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.store_alert(alert.clone());
        alert
    }

    #[cfg(test)]
    fn nearby_recipients(&self, lat: f64, lng: f64, radius_km: f64) -> Vec<AlertRecipient> {
        let subscriptions = self.subscriptions.lock().expect("notification lock");
        let mut recipients: Vec<_> = subscriptions
            .values()
            .filter_map(|subscription| {
                let distance = haversine_km(lat, lng, subscription.lat, subscription.lng);
                let effective_radius = radius_km.min(subscription.radius_km);
                (distance <= effective_radius).then_some(AlertRecipient {
                    user_id: subscription.user_id.clone(),
                    push_token: subscription.push_token.clone(),
                    platform: subscription.platform.clone(),
                    distance_km: (distance * 10.0).round() / 10.0,
                    delivery_status: "queued",
                })
            })
            .collect();
        recipients.sort_by(|a, b| a.distance_km.total_cmp(&b.distance_km));
        recipients
    }

    #[cfg(test)]
    fn store_alert(&self, alert: RescueAlert) {
        let mut alerts = self.recent_alerts.lock().expect("notification alerts lock");
        alerts.push_front(alert);
        while alerts.len() > MAX_RECENT_ALERTS {
            alerts.pop_back();
        }
    }
}

pub fn push_platform_as_str(platform: &PushPlatform) -> &'static str {
    match platform {
        PushPlatform::Ios => "ios",
        PushPlatform::Android => "android",
        PushPlatform::Expo => "expo",
        PushPlatform::Web => "web",
    }
}

pub fn push_platform_from_str(value: &str) -> PushPlatform {
    match value {
        "ios" => PushPlatform::Ios,
        "android" => PushPlatform::Android,
        "web" => PushPlatform::Web,
        _ => PushPlatform::Expo,
    }
}

pub async fn upsert_persistent_subscription(
    db: &PgPool,
    user_id: Uuid,
    subscription: &PushSubscription,
) -> Result<usize, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO push_subscriptions (
          user_id, push_token, platform, lat, lng, radius_km, critical_alerts, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, now())
        ON CONFLICT (push_token)
        DO UPDATE SET
          user_id = EXCLUDED.user_id,
          platform = EXCLUDED.platform,
          lat = EXCLUDED.lat,
          lng = EXCLUDED.lng,
          radius_km = EXCLUDED.radius_km,
          critical_alerts = EXCLUDED.critical_alerts,
          updated_at = now()
        "#,
    )
    .bind(user_id)
    .bind(&subscription.push_token)
    .bind(push_platform_as_str(&subscription.platform))
    .bind(subscription.lat)
    .bind(subscription.lng)
    .bind(subscription.radius_km)
    .bind(subscription.critical_alerts)
    .execute(db)
    .await?;

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM push_subscriptions")
        .fetch_one(db)
        .await?;
    Ok(count as usize)
}

pub async fn dispatch_persistent_rescue_alert(
    db: &PgPool,
    post: &Post,
    default_radius_km: f64,
) -> Result<RescueAlert, sqlx::Error> {
    let radius_km = if post.urgent { 8.0 } else { default_radius_km }.clamp(1.0, 50.0);
    let title = if post.urgent {
        "Resgate urgente perto de voce".to_string()
    } else {
        "Animal precisa de ajuda perto de voce".to_string()
    };
    let body = format!(
        "{} em {}. Toque para ir ao local ou apoiar pelo chat.",
        post.name, post.neighborhood
    );

    let rows = sqlx::query(
        r#"
        SELECT user_id, push_token, platform, lat, lng, radius_km
        FROM push_subscriptions
        WHERE updated_at > now() - interval '90 days'
        "#,
    )
    .fetch_all(db)
    .await?;

    let mut recipients: Vec<_> = rows
        .into_iter()
        .filter_map(|row| {
            let subscription_radius: f64 = row.get("radius_km");
            let distance = haversine_km(
                post.latitude,
                post.longitude,
                row.get("lat"),
                row.get("lng"),
            );
            let effective_radius = radius_km.min(subscription_radius);
            (distance <= effective_radius).then_some(AlertRecipient {
                user_id: row.get::<Uuid, _>("user_id").to_string(),
                push_token: row.get("push_token"),
                platform: push_platform_from_str(row.get::<&str, _>("platform")),
                distance_km: (distance * 10.0).round() / 10.0,
                delivery_status: "queued",
            })
        })
        .collect();
    recipients.sort_by(|a, b| a.distance_km.total_cmp(&b.distance_km));

    let alert = RescueAlert {
        id: Uuid::now_v7().to_string(),
        post_id: post.id.clone(),
        title,
        body,
        image_url: post.image.clone(),
        lat: post.latitude,
        lng: post.longitude,
        radius_km,
        critical: post.urgent,
        actions: vec![
            NotificationAction {
                id: "go_to_location",
                label: "Ir ao local",
                deep_link: format!("zoohelp://post/{}?action=route", post.id),
            },
            NotificationAction {
                id: "remote_support",
                label: "Apoiar remoto",
                deep_link: format!("zoohelp://post/{}?action=chat", post.id),
            },
        ],
        recipients,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    persist_rescue_alert(db, &alert).await?;
    Ok(alert)
}

pub async fn persist_rescue_alert(db: &PgPool, alert: &RescueAlert) -> Result<(), sqlx::Error> {
    if alert.recipients.is_empty() {
        sqlx::query(
            r#"
            INSERT INTO notification_events (
              kind, title, body, post_id, image_url, critical, deeplink,
              dedupe_key, ttl_seconds, category, payload
            )
            VALUES ('rescue_alert', $1, $2, $3, $4, $5, $6, $7, 900, 'rescue', $8)
            "#,
        )
        .bind(&alert.title)
        .bind(&alert.body)
        .bind(&alert.post_id)
        .bind(alert.image_url.as_deref())
        .bind(alert.critical)
        .bind(format!("zoohelp://post/{}", alert.post_id))
        .bind(format!("rescue:{}", alert.post_id))
        .bind(serde_json::json!({ "alertId": alert.id, "radiusKm": alert.radius_km }))
        .execute(db)
        .await?;
        return Ok(());
    }

    for recipient in &alert.recipients {
        let user_id = Uuid::parse_str(&recipient.user_id).ok();
        let event_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO notification_events (
              user_id, kind, title, body, post_id, image_url, distance_km, critical,
              deeplink, dedupe_key, ttl_seconds, category, payload
            )
            VALUES ($1, 'rescue_alert', $2, $3, $4, $5, $6, $7, $8, $9, 900, 'rescue', $10)
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(&alert.title)
        .bind(&alert.body)
        .bind(&alert.post_id)
        .bind(alert.image_url.as_deref())
        .bind(recipient.distance_km)
        .bind(alert.critical)
        .bind(format!("zoohelp://post/{}", alert.post_id))
        .bind(format!("rescue:{}", alert.post_id))
        .bind(serde_json::json!({
            "alertId": alert.id,
            "pushToken": recipient.push_token,
            "platform": push_platform_as_str(&recipient.platform),
            "deliveryStatus": recipient.delivery_status,
            "radiusKm": alert.radius_km
        }))
        .fetch_one(db)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO push_delivery_jobs (
              notification_event_id, user_id, push_token, platform, payload
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(event_id)
        .bind(user_id)
        .bind(&recipient.push_token)
        .bind(push_platform_as_str(&recipient.platform))
        .bind(serde_json::json!({
            "title": alert.title,
            "body": alert.body,
            "deeplink": format!("zoohelp://post/{}", alert.post_id),
            "postId": alert.post_id,
            "critical": alert.critical
        }))
        .execute(db)
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::seed_posts;

    #[test]
    fn dispatches_only_to_nearby_subscribers() {
        let engine = NotificationEngine::default();
        engine.upsert_subscription(PushSubscription {
            user_id: "near".into(),
            push_token: "ExponentPushToken[near]".into(),
            platform: PushPlatform::Expo,
            lat: -23.5506,
            lng: -46.6334,
            radius_km: 10.0,
            critical_alerts: true,
            updated_at: "now".into(),
        });
        engine.upsert_subscription(PushSubscription {
            user_id: "far".into(),
            push_token: "ExponentPushToken[far]".into(),
            platform: PushPlatform::Expo,
            lat: -22.9068,
            lng: -43.1729,
            radius_km: 10.0,
            critical_alerts: true,
            updated_at: "now".into(),
        });

        let post = seed_posts()
            .into_iter()
            .find(|post| post.urgent)
            .expect("urgent seed post");

        let alert = engine.dispatch_rescue_alert(&post, 5.0);

        assert_eq!(alert.recipients.len(), 1);
        assert_eq!(alert.recipients[0].user_id, "near");
        assert!(alert.critical);
    }
}
