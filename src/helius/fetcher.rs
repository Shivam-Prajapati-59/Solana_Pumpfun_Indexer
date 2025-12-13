use anyhow::Result;
use futures_util::StreamExt;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};

use crate::db::get_db_pool;
use crate::helius::parser::{parse_pump_fun_transaction, parse_token_creation};
use crate::models::queries::{
    get_token, get_token_holder, insert_trade, upsert_token, upsert_token_holder,
};
use crate::models::{Token, TokenHolder, helius_model::TransactionResult};
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
                            eprintln!("DB Error: {}", e);
                        }
                    }
                    Err(e) => eprintln!(" Fetch Error for {}: {}", signature, e),
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

    // 1. Check if this is a token creation event
    if let Ok(Some(mut token)) = parse_token_creation(&tx) {
        println!("🪙 New token created: {}", token.mint_address);

        // Calculate initial market cap
        if !token.virtual_token_reserves.is_zero() {
            let token_price_sol = token.virtual_sol_reserves / token.virtual_token_reserves;
            let sol_price_dec = Decimal::from_f64(current_sol_price).unwrap_or(Decimal::ZERO);
            token.market_cap_usd = token_price_sol * token.token_total_supply * sol_price_dec;
        }

        upsert_token(pool, &token).await?;
        println!(
            "✅ Token saved to DB (Market Cap: ${:.2})",
            token.market_cap_usd
        );
    }

    // 2. Parse the transaction for trades
    match parse_pump_fun_transaction(&tx, current_sol_price) {
        Ok(Some(trade)) => {
            println!(
                "💰 Trade detected: {} {} tokens for {} SOL (${:.2})",
                if trade.is_buy { "BUY" } else { "SELL" },
                trade.token_amount,
                trade.sol_amount,
                trade.price_usd.unwrap_or(Decimal::ZERO)
            );

            // Insert the trade
            insert_trade(pool, &trade).await?;
            println!("✅ Trade saved to DB: {}", sig);

            // 3. Update token with latest reserves and market cap (or create if not exists)
            let mut token = match get_token(pool, &trade.token_mint).await {
                Ok(Some(t)) => t,
                _ => {
                    // Token doesn't exist, try to extract metadata from this transaction
                    let (name, symbol, uri) = extract_token_metadata_from_tx(&tx);
                    let bonding_curve = find_bonding_curve_from_tx(&tx, &trade.token_mint);

                    Token {
                        mint_address: trade.token_mint.clone(),
                        name,
                        symbol,
                        uri,
                        bonding_curve_address: bonding_curve,
                        creator_wallet: Some(trade.user_wallet.clone()),
                        virtual_token_reserves: trade.virtual_token_reserves,
                        virtual_sol_reserves: trade.virtual_sol_reserves,
                        real_token_reserves: Decimal::ZERO,
                        token_total_supply: Decimal::from_u64(1_000_000_000_000_000)
                            .unwrap_or(Decimal::ZERO), // 1B tokens standard
                        market_cap_usd: Decimal::ZERO,
                        bonding_curve_progress: Decimal::ZERO,
                        complete: false,
                        created_at: trade.timestamp,
                        updated_at: None,
                    }
                }
            };

            // If token metadata is missing, try to extract from current transaction
            if token.name.is_none() || token.symbol.is_none() || token.uri.is_none() {
                let (name, symbol, uri) = extract_token_metadata_from_tx(&tx);
                if token.name.is_none() && name.is_some() {
                    token.name = name;
                }
                if token.symbol.is_none() && symbol.is_some() {
                    token.symbol = symbol;
                }
                if token.uri.is_none() && uri.is_some() {
                    token.uri = uri;
                }
            }

            // If bonding curve is missing, try to find it
            if token.bonding_curve_address.is_none() {
                token.bonding_curve_address = find_bonding_curve_from_tx(&tx, &trade.token_mint);
            }

            // Update reserves from trade
            token.virtual_sol_reserves = trade.virtual_sol_reserves;
            token.virtual_token_reserves = trade.virtual_token_reserves;

            // Calculate market cap: (price_per_token) * total_supply
            if !trade.virtual_token_reserves.is_zero() {
                let token_price_sol = trade.virtual_sol_reserves / trade.virtual_token_reserves;
                let sol_price_dec = Decimal::from_f64(current_sol_price).unwrap_or(Decimal::ZERO);
                token.market_cap_usd = token_price_sol * token.token_total_supply * sol_price_dec;

                // Calculate bonding curve progress (typically completes at 85 SOL)
                let bonding_complete_sol =
                    Decimal::from_u64(85_000_000_000).unwrap_or(Decimal::ZERO); // 85 SOL in lamports
                token.bonding_curve_progress =
                    (trade.virtual_sol_reserves / bonding_complete_sol) * Decimal::from(100);
                if token.bonding_curve_progress >= Decimal::from(100) {
                    token.complete = true;
                }
            }

            upsert_token(pool, &token).await?;
            println!(
                "✅ Token saved/updated (Market Cap: ${:.2}, Progress: {:.1}%)",
                token.market_cap_usd, token.bonding_curve_progress
            );

            // 4. Update token holder balance
            // Get current balance if exists
            let current_balance = if let Ok(Some(holder)) =
                get_token_holder(pool, &trade.token_mint, &trade.user_wallet).await
            {
                holder.balance
            } else {
                Decimal::ZERO
            };

            // Calculate new balance
            let new_balance = if trade.is_buy {
                current_balance + trade.token_amount
            } else {
                // For sells, ensure we don't go negative
                if current_balance >= trade.token_amount {
                    current_balance - trade.token_amount
                } else {
                    Decimal::ZERO
                }
            };

            // Update holder only if balance > 0 or it's a buy
            if new_balance > Decimal::ZERO || trade.is_buy {
                let holder = TokenHolder {
                    token_mint: trade.token_mint.clone(),
                    user_wallet: trade.user_wallet.clone(),
                    balance: new_balance,
                    last_updated_slot: trade.slot,
                    updated_at: None,
                };

                upsert_token_holder(pool, &holder).await?;
                println!(
                    "✅ Token holder updated: {} (balance: {})",
                    trade.user_wallet, new_balance
                );
            } else {
                println!("ℹ️  Holder {} sold all tokens", trade.user_wallet);
            }
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

/// Extract token metadata from transaction logs
fn extract_token_metadata_from_tx(
    tx: &TransactionResult,
) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(meta) = &tx.meta {
        if let Some(logs) = &meta.log_messages {
            let mut name = None;
            let mut symbol = None;
            let mut uri = None;

            for log in logs {
                // Extract name
                if name.is_none() && (log.contains("name:") || log.contains("Name:")) {
                    if let Some(pos) = log.find("name:").or_else(|| log.find("Name:")) {
                        let start = pos + 5;
                        let value = log[start..]
                            .trim()
                            .split(&[',', '"', '}'][..])
                            .next()
                            .unwrap_or("")
                            .trim();
                        if !value.is_empty() && value.len() < 100 {
                            name = Some(value.to_string());
                        }
                    }
                }

                // Extract symbol
                if symbol.is_none() && (log.contains("symbol:") || log.contains("Symbol:")) {
                    if let Some(pos) = log.find("symbol:").or_else(|| log.find("Symbol:")) {
                        let start = pos + 7;
                        let value = log[start..]
                            .trim()
                            .split(&[',', '"', '}'][..])
                            .next()
                            .unwrap_or("")
                            .trim();
                        if !value.is_empty() && value.len() < 20 {
                            symbol = Some(value.to_string());
                        }
                    }
                }

                // Extract URI
                if uri.is_none()
                    && (log.contains("uri:") || log.contains("Uri:") || log.contains("metadata:"))
                {
                    if let Some(pos) = log
                        .find("uri:")
                        .or_else(|| log.find("Uri:"))
                        .or_else(|| log.find("metadata:"))
                    {
                        let start = pos
                            + if log[pos..].starts_with("metadata:") {
                                9
                            } else {
                                4
                            };
                        let value = log[start..]
                            .trim()
                            .split(&[' ', ',', '"', '}'][..])
                            .next()
                            .unwrap_or("")
                            .trim();
                        if value.starts_with("http")
                            || value.starts_with("ipfs")
                            || value.starts_with("ar://")
                        {
                            uri = Some(value.to_string());
                        }
                    }
                }
            }

            return (name, symbol, uri);
        }
    }
    (None, None, None)
}

/// Find bonding curve address from transaction accounts
/// Pump.fun bonding curve characteristics:
/// - Writable PDA at specific position (typically 4-6)
/// - Holds SOL balance for the curve
/// - Not a signer or known program
fn find_bonding_curve_from_tx(tx: &TransactionResult, mint_address: &str) -> Option<String> {
    const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
    const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
    const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
    const RENT_PROGRAM: &str = "SysvarRent111111111111111111111111111111111";
    const EVENT_AUTHORITY: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";

    let mut candidates = Vec::new();

    for (idx, account) in tx.transaction.message.account_keys.iter().enumerate() {
        // Must be writable and not a signer
        if !account.writable || account.signer {
            continue;
        }

        // Skip known addresses
        if account.pubkey == mint_address
            || account.pubkey == SYSTEM_PROGRAM
            || account.pubkey == TOKEN_PROGRAM
            || account.pubkey == ASSOCIATED_TOKEN_PROGRAM
            || account.pubkey == RENT_PROGRAM
            || account.pubkey == EVENT_AUTHORITY
            || account.pubkey == SOL_MINT
            || account.pubkey == PUMP_FUN_PROGRAM_ID
        {
            continue;
        }

        // Bonding curve typically at position 4-6
        if idx >= 3 && idx <= 7 && account.pubkey.len() == 44 {
            candidates.push((idx, account.pubkey.clone()));
        }
    }

    // Return first valid candidate
    candidates.first().map(|(_, addr)| addr.clone())
}
