use anyhow::{Context, Result};
use redis::Client;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::env;
use std::time::Duration;

/// Creates a connection pool to the Postgres database
pub async fn get_db_pool() -> Result<PgPool> {
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set in .env")?;

    PgPoolOptions::new()
        .connect(&database_url)
        .await
        .context("Failed to connect to Postgres")
}

/// Creates a Redis client instance.
pub fn get_redis_client() -> Result<Client> {
    let redis_url = env::var("REDIS_URL").context("REDIS_URL must be set in .env")?;

    let client = Client::open(redis_url).context("Failed to parse Redis URL")?;

    Ok(client)
}

/// Helper to get an async connection directly (used in workers)
pub async fn get_redis_conn(client: &Client) -> Result<redis::aio::MultiplexedConnection> {
    client
        .get_multiplexed_async_connection()
        .await
        .context("Failed to get async Redis connection")
}
