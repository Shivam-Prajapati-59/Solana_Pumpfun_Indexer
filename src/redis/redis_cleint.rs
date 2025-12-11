use anyhow::{Context, Result};
use futures_util::StreamExt;
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, Client, RedisResult};
use serde_json;
use std::fmt;
use tracing::{info, warn};

#[derive(Clone)]
pub struct RedisClient {
    pub connection: ConnectionManager,
    client: Client,
}

impl fmt::Debug for RedisClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisClient")
            .field("connection", &"ConnectionManager<Connected>")
            .finish()
    }
}

impl RedisClient {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url).context("Failed to create Redis client")?;

        let connection = ConnectionManager::new(client.clone())
            .await
            .context("Failed to establish Redis connection")?;

        info!("Successfully connected to Redis");

        Ok(Self { connection, client })
    }

    pub async fn publish<T: serde::Serialize>(&mut self, channel: &str, message: &T) -> Result<()> {
        let json = serde_json::to_string(message).context("Failed to serialize message")?;

        self.connection
            .publish::<_, _, ()>(channel, json)
            .await
            .map_err(|e| {
                warn!("Redis publish error: {}", e);
                if e.is_connection_dropped() || e.is_io_error() {
                    warn!("Redis connection lost, attempting reconnect...");
                }
                anyhow::Error::from(e)
            })?;

        Ok(())
    }

    // --- NEW: Subscribe Method for the Worker ---
    pub async fn subscribe(
        &self,
        channel: &str,
    ) -> Result<impl futures_util::Stream<Item = String>> {
        let mut pubsub_conn = self
            .client
            .get_async_pubsub()
            .await
            .context("Failed to get PubSub")?;
        pubsub_conn.subscribe(channel).await?;

        // Transform the stream to return just the payload string
        let stream = pubsub_conn
            .into_on_message()
            .map(|msg| msg.get_payload::<String>().unwrap_or_default());

        Ok(stream)
    }

    pub async fn set<T: serde::Serialize>(
        &mut self,
        key: &str,
        value: &T,
        expiry_seconds: Option<usize>,
    ) -> Result<()> {
        let json = serde_json::to_string(value).context("Failed to serialize value")?;

        if let Some(seconds) = expiry_seconds {
            self.connection
                .set_ex::<_, _, ()>(key, json, seconds as u64)
                .await
                .context("Failed to set key with expiry")?;
        } else {
            self.connection
                .set::<_, _, ()>(key, json)
                .await
                .context("Failed to set key")?;
        }

        Ok(())
    }

    pub async fn get<T: serde::de::DeserializeOwned>(&mut self, key: &str) -> Result<Option<T>> {
        let result: RedisResult<String> = self.connection.get(key).await;

        match result {
            Ok(json) => {
                let value = serde_json::from_str(&json).context("Failed to deserialize value")?;
                Ok(Some(value))
            }
            Err(e) if e.kind() == redis::ErrorKind::TypeError => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn delete(&mut self, key: &str) -> Result<()> {
        self.connection
            .del::<_, ()>(key)
            .await
            .context("Failed to delete key")?;

        Ok(())
    }

    pub async fn increment(&mut self, key: &str) -> Result<i64> {
        let value = self
            .connection
            .incr(key, 1)
            .await
            .context("Failed to increment counter")?;

        Ok(value)
    }

    pub async fn ping(&mut self) -> Result<()> {
        // Simple ping test - set and get a test value
        let _: () = self
            .connection
            .set("ping_test", "pong")
            .await
            .context("Redis PING failed")?;
        Ok(())
    }
}
