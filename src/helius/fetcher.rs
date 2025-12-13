use anyhow::Result;
use futures_util::StreamExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};

use crate::db::get_db_pool;
use crate::helius::parser::parse_pump_fun_transaction;
use crate::models::helius_model::TransactionResult;
use crate::models::queries::insert_trade;
use crate::redis::redis_cleint::RedisClient;

const REDIS_CHANNEL: &str = "solana:transactions";
const SOL_USD_FEED_ID: &str = "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d";
const PRICE_CACHE_TTL_SECS: u64 = 30; // Cache price for 30 seconds

#[derive(Clone)]
struct PriceCache {
    price: f64,
    updated_at: Instant,
}

pub async fn run_worker() -> Result<()> {
    let api_key = std::env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY missing");
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    // Initialize database pool
    let db_pool = get_db_pool().await?;
    println!("✅ Database connected");

    let redis = RedisClient::new(&redis_url).await?;
    let http_client = reqwest::Client::new();

    // Initialize price cache with shared state
    let price_cache = Arc::new(RwLock::new(None::<PriceCache>));

    println!("🎧 Worker started. Listening on: {}", REDIS_CHANNEL);

    // Subscribe to the channel
    let mut stream = redis.subscribe(REDIS_CHANNEL).await?;

    // Reactive Loop: Code waits here until Redis sends a message
    while let Some(payload) = stream.next().await {
        println!("⚡ Event Received: {}", payload);

        // Parse the mini-info (Signature) from Redis
        if let Ok(info) = serde_json::from_str::<Value>(&payload) {
            if let Some(signature) = info.get("signature").and_then(|s| s.as_str()) {
                println!("🔍 Fetching details for: {}", signature);

                // Fetch full data from RPC
                match fetch_full_transaction(&http_client, &api_key, signature).await {
                    Ok(tx) => {
                        // Parse and save to Database
                        if let Err(e) =
                            process_and_save(&db_pool, &http_client, &price_cache, tx).await
                        {
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

/// Fetch current SOL/USD price from Pyth Network with caching
async fn get_sol_price(
    client: &reqwest::Client,
    cache: &Arc<RwLock<Option<PriceCache>>>,
) -> Result<f64> {
    // Check cache first
    {
        let cached = cache.read().await;
        if let Some(price_data) = cached.as_ref() {
            let age = price_data.updated_at.elapsed();
            if age.as_secs() < PRICE_CACHE_TTL_SECS {
                return Ok(price_data.price);
            }
        }
    }

    // Fetch new price from Pyth Hermes API
    let url = format!(
        "https://hermes.pyth.network/v2/updates/price/latest?ids[]={}",
        SOL_USD_FEED_ID
    );

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("Pyth API error: {}", response.status()));
    }

    let data: Value = response.json().await?;

    // Parse Pyth response
    if let Some(parsed) = data.get("parsed").and_then(|p| p.as_array()) {
        if let Some(first_price) = parsed.first() {
            if let Some(price_data) = first_price.get("price") {
                let price_str = price_data.get("price").and_then(|p| p.as_str());
                let expo = price_data.get("expo").and_then(|e| e.as_i64());

                if let (Some(price), Some(exp)) = (price_str, expo) {
                    if let Ok(price_int) = price.parse::<i64>() {
                        // Calculate actual price: price * 10^expo
                        let actual_price = (price_int as f64) * 10f64.powi(exp as i32);

                        // Update cache
                        {
                            let mut cached = cache.write().await;
                            *cached = Some(PriceCache {
                                price: actual_price,
                                updated_at: Instant::now(),
                            });
                        }

                        println!("💵 SOL Price: ${:.2}", actual_price);
                        return Ok(actual_price);
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!("Failed to parse Pyth price data"))
}

async fn process_and_save(
    pool: &PgPool,
    http_client: &reqwest::Client,
    price_cache: &Arc<RwLock<Option<PriceCache>>>,
    tx: TransactionResult,
) -> Result<()> {
    let sig = tx
        .transaction
        .signatures
        .first()
        .cloned()
        .unwrap_or_default();

    println!("📊 Processing transaction: {}", sig);

    // Get real-time SOL price with caching
    let current_sol_price = get_sol_price(http_client, price_cache).await?;

    // Parse the transaction
    match parse_pump_fun_transaction(&tx, current_sol_price) {
        Ok(Some(trade)) => {
            println!(
                "💰 Trade detected: {} {} tokens for {} SOL",
                if trade.is_buy { "BUY" } else { "SELL" },
                trade.token_amount,
                trade.sol_amount
            );

            // Insert into database
            insert_trade(pool, &trade).await?;
            println!("✅ Trade saved to DB: {}", sig);
        }
        Ok(None) => {
            println!("ℹ️  No trade data found in transaction");
        }
        Err(e) => {
            eprintln!("⚠️  Parse error for {}: {}", sig, e);
        }
    }

    Ok(())
}
