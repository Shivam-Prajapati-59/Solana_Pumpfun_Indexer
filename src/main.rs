mod helius;
mod models;
mod redis;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    println!("🚀 Starting Pump.fun Indexer - WebSocket Ingester");
    println!("===================================================\n");

    helius::ingester::run_ingester().await?;
    Ok(())
}
