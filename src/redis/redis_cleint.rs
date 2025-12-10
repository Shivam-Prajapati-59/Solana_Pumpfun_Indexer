use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, Client, RedisResult};
use serde_json::{self};
use tracing::{info, warn};

#[derive(Clone)]
pub struct RedisClient {
    pub connection: ConnectionManager,
}

impl RedisClient {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url).context("Failed to create Redis client")?;

        let connection = ConnectionManager::new(client)
            .await
            .context("Failed to establish Redis connection")?;

        info!("Successfully connected to Redis");

        Ok(Self { connection })
    }

    pub async fn publish<T: serde::Serialize>(&mut self, channel: &str, message: &T) -> Result<()> {
        let json = serde_json::to_string(message).context("Failed to serialize message")?;

        match self
            .connection
            .publish::<_, _, ()>(channel, json.clone())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!("  Redis publish error: {}", e);

                if e.is_connection_dropped() || e.is_io_error() {
                    warn!("  Redis connection lost, attempting reconnect...");
                }

                Err(e.into())
            }
        }
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
        redis::cmd("PING")
            .query_async::<String>(&mut self.connection)
            .await
            .context("Redis PING failed")?;
        Ok(())
    }
}
