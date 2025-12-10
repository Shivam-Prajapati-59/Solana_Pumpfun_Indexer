use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, Client, RedisResult};
use serde_json::{self, json, value};
use tracing::{error, info, warn};

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

    pub async fn publish<T: serde::Serialize>(&mut self, channel: &str, meassage: &T) {
        let json_message =
            serde_json::to_string(meassage).context("Failed to Serialize Message into the Json");

        match self
            .connection
            .publish::<_, _, ()>(channel, json_message.clone().await)
        {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!("Redis Push Error : {}", e);
                if e.is_connection_dropped() || e.is_io_error() {
                    warn!(" Redis connection lost, attempting reconnect...");
                }
                Err(e.into())
            }
        }
    }

    pub async fn set<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        expiry_seconds: Option<u64>,
    ) -> Result<()> {
        let json = serde_json::to_string(value)?;

        let mut cmd = redis::cmd("SET");
        cmd.arg(key).arg(json);

        if let Some(ttl) = expiry_seconds {
            cmd.arg("EX").arg(ttl);
        }

        cmd.query_async::<(), _>(&mut self.connection).await?;
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
