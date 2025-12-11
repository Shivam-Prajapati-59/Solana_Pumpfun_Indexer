use crate::redis::redis_cleint::{self, RedisClient};

mod helius;
mod models;
mod redis;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables
    dotenvy::dotenv().ok();

    println!("🚀 Starting Pump.fun Indexer - WebSocket Test");
    println!("=============================================\n");

    // Run the WebSocket test
    helius::ingester::run_ingester().await?;

    Ok(())
}
