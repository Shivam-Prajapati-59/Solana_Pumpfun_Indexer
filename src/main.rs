use std::env;

use sqlx::{Connection, PgConnection};

mod helius;
mod models;
mod redis;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let db_url = env::var("DATABASE_URL")?;

    // Create connection pool
    let mut pool = PgConnection::connect(&db_url).await?;

    sqlx::migrate!("./migrations").run(&mut pool).await?;
    println!("🚀 Starting Pump.fun Indexer - WebSocket Ingester");
    println!("===================================================\n");

    helius::ingester::run_ingester().await?;
    Ok(())
}
