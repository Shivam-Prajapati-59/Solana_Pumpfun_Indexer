use indexer::helius::fetcher;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    fetcher::run_worker().await?;
    Ok(())
}
