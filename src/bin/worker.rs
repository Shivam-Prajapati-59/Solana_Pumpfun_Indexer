use indexer::helius::fetcher;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    
    println!("🎧 Starting Pump.fun Indexer - Worker");
    println!("======================================\n");
    
    fetcher::run_worker().await?;
    Ok(())
}
