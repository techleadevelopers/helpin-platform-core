use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use crate::{domain::Post, services::geo::haversine_km};

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

#[derive(Clone, Default)]
pub struct NotificationEngine {
    subscriptions: Arc<Mutex<HashMap<String, PushSubscription>>>,
    recent_alerts: Arc<Mutex<VecDeque<RescueAlert>>>,
}

impl NotificationEngine {
    pub fn upsert_subscription(&self, subscription: PushSubscription) -> usize {
        let mut subscriptions = self.subscriptions.lock().expect("notification lock");
        subscriptions.insert(subscription.push_token.clone(), subscription);
        subscriptions.len()
    }

    pub fn list_recent_alerts(&self) -> Vec<RescueAlert> {
        self.recent_alerts
            .lock()
            .expect("notification alerts lock")
            .iter()
            .cloned()
            .collect()
    }

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

    fn store_alert(&self, alert: RescueAlert) {
        let mut alerts = self.recent_alerts.lock().expect("notification alerts lock");
        alerts.push_front(alert);
        while alerts.len() > MAX_RECENT_ALERTS {
            alerts.pop_back();
        }
    }
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
