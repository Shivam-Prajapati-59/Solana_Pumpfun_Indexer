use crate::models::helius_model::TransactionResult; // Ensure you have this model
use crate::redis::redis_cleint::RedisClient;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::time::Duration;

const REDIS_CHANNEL: &str = "solana:transactions";

pub async fn run_worker() -> Result<()> {
    let api_key = std::env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY missing");
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let redis = RedisClient::new(&redis_url).await?;
    let http_client = reqwest::Client::new();

    println!("🎧 Worker started. Listening on: {}", REDIS_CHANNEL);

    // 1. Subscribe to the channel
    let mut stream = redis.subscribe(REDIS_CHANNEL).await?;

    // 2. Reactive Loop: Code waits here until Redis sends a message
    while let Some(payload) = stream.next().await {
        println!("⚡ Event Received: {}", payload);

        // Parse the mini-info (Signature) from Redis
        if let Ok(info) = serde_json::from_str::<Value>(&payload) {
            if let Some(signature) = info.get("signature").and_then(|s| s.as_str()) {
                println!("🔍 Fetching details for: {}", signature);

                // Fetch full data from RPC
                match fetch_full_transaction(&http_client, &api_key, signature).await {
                    Ok(tx) => {
                        // Save to Database
                        if let Err(e) = save_to_database(tx).await {
                            eprintln!("❌ DB Error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("❌ Fetch Error for {}: {}", signature, e),
                }
            }
        }
    }

    Ok(())
}

async fn fetch_full_transaction(
    client: &reqwest::Client,
    api_key: &str,
    signature: &str,
) -> Result<TransactionResult> {
    // Basic Helius RPC call with retries
    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", api_key);
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [ signature, { "encoding": "jsonParsed", "maxSupportedTransactionVersion": 0 } ]
    });

    // Simple retry logic (3 attempts)
    for attempt in 1..=3 {
        let resp = client.post(&rpc_url).json(&request).send().await?;
        if resp.status() == 429 {
            tokio::time::sleep(Duration::from_secs(attempt)).await;
            continue;
        }

        let body: Value = resp.json().await?;
        if let Some(result) = body.get("result") {
            if !result.is_null() {
                let tx: TransactionResult = serde_json::from_value(result.clone())?;
                return Ok(tx);
            }
        }

        // If result is null (not indexed yet), wait a bit and retry
        tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
    }

    Err(anyhow::anyhow!("Transaction not found after retries"))
}

async fn save_to_database(tx: TransactionResult) -> Result<()> {
    let sig = tx
        .transaction
        .signatures
        .first()
        .cloned()
        .unwrap_or_default();

    // TODO: Write your Diesel / SQLx insert code here
    println!("💾 [DB] Successfully saved transaction: {}", sig);

    Ok(())
}
