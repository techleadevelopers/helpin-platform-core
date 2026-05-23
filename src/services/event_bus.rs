use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::Config,
    state::{ChatEvent, FeedEvent, RescueEvent},
};

pub const CHAT_MESSAGES_SUBJECT: &str = "zoohelp.chat.messages";
pub const RESCUE_EVENTS_SUBJECT: &str = "zoohelp.rescue.events";
pub const FEED_EVENTS_SUBJECT: &str = "zoohelp.feed.events";

#[derive(Clone)]
pub struct EventBus {
    client: Option<async_nats::Client>,
    origin_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventEnvelope<T> {
    origin_id: String,
    event: T,
}

impl EventBus {
    pub async fn connect(config: &Config) -> anyhow::Result<Self> {
        let origin_id = Uuid::now_v7().to_string();
        if config.app_env == "test" {
            return Ok(Self {
                client: None,
                origin_id,
            });
        }
        match async_nats::connect(&config.nats_url).await {
            Ok(client) => Ok(Self {
                client: Some(client),
                origin_id,
            }),
            Err(error) if config.is_development() => {
                tracing::warn!(
                    ?error,
                    "NATS unavailable; realtime is local-process only in development"
                );
                Ok(Self {
                    client: None,
                    origin_id,
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.client.is_some()
    }

    pub async fn publish_chat(&self, event: &ChatEvent) {
        self.publish(CHAT_MESSAGES_SUBJECT, event).await;
    }

    pub async fn publish_rescue(&self, event: &RescueEvent) {
        self.publish(RESCUE_EVENTS_SUBJECT, event).await;
    }

    pub async fn publish_feed(&self, event: &FeedEvent) {
        self.publish(FEED_EVENTS_SUBJECT, event).await;
    }

    pub fn spawn_bridge(
        &self,
        chat_tx: tokio::sync::broadcast::Sender<ChatEvent>,
        rescue_tx: tokio::sync::broadcast::Sender<RescueEvent>,
        feed_tx: tokio::sync::broadcast::Sender<FeedEvent>,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };

        spawn_subscription(
            client.clone(),
            self.origin_id.clone(),
            CHAT_MESSAGES_SUBJECT,
            move |event| {
                let _ = chat_tx.send(event);
            },
        );
        spawn_subscription(
            client.clone(),
            self.origin_id.clone(),
            RESCUE_EVENTS_SUBJECT,
            move |event| {
                let _ = rescue_tx.send(event);
            },
        );
        spawn_subscription(
            client,
            self.origin_id.clone(),
            FEED_EVENTS_SUBJECT,
            move |event| {
                let _ = feed_tx.send(event);
            },
        );
    }

    async fn publish<T>(&self, subject: &'static str, event: &T)
    where
        T: Serialize,
    {
        let Some(client) = &self.client else {
            return;
        };
        let envelope = EventEnvelope {
            origin_id: self.origin_id.clone(),
            event,
        };
        match serde_json::to_vec(&envelope) {
            Ok(payload) => {
                if let Err(error) = client.publish(subject, payload.into()).await {
                    tracing::warn!(?error, subject, "event bus publish failed");
                }
            }
            Err(error) => tracing::warn!(?error, subject, "event serialization failed"),
        }
    }
}

fn spawn_subscription<T>(
    client: async_nats::Client,
    origin_id: String,
    subject: &'static str,
    handler: impl Fn(T) + Send + Sync + 'static,
) where
    T: DeserializeOwned + Send + 'static,
{
    tokio::spawn(async move {
        let mut subscriber = match client.subscribe(subject).await {
            Ok(subscriber) => subscriber,
            Err(error) => {
                tracing::warn!(?error, subject, "event bus subscribe failed");
                return;
            }
        };

        while let Some(message) = futures_util::StreamExt::next(&mut subscriber).await {
            let envelope = serde_json::from_slice::<EventEnvelope<T>>(&message.payload);
            match envelope {
                Ok(envelope) if envelope.origin_id != origin_id => handler(envelope.event),
                Ok(_) => {}
                Err(error) => tracing::debug!(?error, subject, "invalid event bus payload"),
            }
        }
    });
}
