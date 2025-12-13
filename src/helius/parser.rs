use crate::models::{Token, Trade, helius_model::TransactionResult};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

// Standard Pump.fun virtual offset (30 SOL)
const VIRTUAL_SOL_OFFSET: u64 = 30 * LAMPORTS_PER_SOL;

/// Parse a Helius transaction and extract trade data
/// `current_sol_price`: Real-time SOL/USD price from your worker cache
pub fn parse_pump_fun_transaction(
    tx: &TransactionResult,
    current_sol_price: f64,
) -> Result<Option<Trade>> {
    // 1. Basic Validation
    let signature = tx
        .transaction
        .signatures
        .first()
        .context("No signature found")?
        .clone();

    // Check if this transaction interacts with Pump.fun
    let is_pump_fun = tx
        .transaction
        .message
        .instructions
        .iter()
        .any(|ix| ix.program_id == PUMP_FUN_PROGRAM_ID);

    if !is_pump_fun {
        return Ok(None);
    }

    // 2. Get Timestamp
    let timestamp = match tx.block_time {
        Some(ts) => DateTime::from_timestamp(ts, 0).unwrap_or(Utc::now()),
        None => Utc::now(),
    };

    // 3. Parse Trade via Balance Changes
    if let Some(meta) = &tx.meta {
        // Safe access to Option<Vec<TokenBalance>>
        let empty_vec = vec![];
        let post_balances = meta.post_token_balances.as_ref().unwrap_or(&empty_vec);
        let pre_balances = meta.pre_token_balances.as_ref().unwrap_or(&empty_vec);

        // Iterate over specific balance entries to find the trade
        for post in post_balances {
            // Ignore Wrapped SOL (we want the pump.fun token)
            if post.mint == SOL_MINT {
                continue;
            }

            // Find the corresponding Pre-Balance
            let pre = pre_balances
                .iter()
                .find(|p| p.account_index == post.account_index && p.mint == post.mint);

            let pre_amount: i64 = pre
                .map(|p| p.ui_token_amount.amount.parse().unwrap_or(0))
                .unwrap_or(0);
            let post_amount: i64 = post.ui_token_amount.amount.parse().unwrap_or(0);

            let diff = post_amount - pre_amount;

            // If balance didn't change, this isn't the trade
            if diff == 0 {
                continue;
            }

            // -------------------------------------------------------
            // FOUND THE TRADE
            // -------------------------------------------------------
            let token_mint = post.mint.clone();
            let user_wallet = post.owner.clone().unwrap_or_default();

            let token_amount_abs = diff.abs() as u64;
            let is_buy = diff > 0; // Balance went UP = Buy, DOWN = Sell

            // 4. Calculate SOL Amount used in the trade
            let sol_amount_abs =
                calculate_sol_change(meta, &user_wallet, &tx.transaction.message.account_keys);

            // 5. Find Bonding Curve Reserves (for tracking market cap/bonding progress)
            let (real_sol_reserves, real_token_reserves) = find_bonding_curve_reserves(
                meta,
                &token_mint,
                post.account_index as i64,
                &tx.transaction.message.account_keys,
            );

            // Calculate Virtual Reserves (Pump.fun constant product formula)
            let virtual_sol = real_sol_reserves + VIRTUAL_SOL_OFFSET;
            let virtual_token = real_token_reserves; // Usually accurate enough for analytics

            // 6. Convert to Decimals
            let decimal_token = Decimal::from_u64(token_amount_abs).unwrap_or(Decimal::ZERO);
            let decimal_sol = Decimal::from_u64(sol_amount_abs).unwrap_or(Decimal::ZERO);

            // 7. Calculate Prices
            let price_sol = if !decimal_token.is_zero() {
                Some(decimal_sol / decimal_token)
            } else {
                Some(Decimal::ZERO)
            };

            let price_usd = price_sol.map(|p| {
                let sol_price_dec = Decimal::from_f64(current_sol_price).unwrap_or(Decimal::ZERO);
                p * sol_price_dec
            });

            return Ok(Some(Trade {
                signature,
                token_mint,
                sol_amount: decimal_sol,
                token_amount: decimal_token,
                is_buy,
                user_wallet,
                timestamp,
                virtual_sol_reserves: Decimal::from_u64(virtual_sol).unwrap_or(Decimal::ZERO),
                virtual_token_reserves: Decimal::from_u64(virtual_token).unwrap_or(Decimal::ZERO),
                price_sol,
                price_usd,
                track_volume: true,
                ix_name: if is_buy {
                    "buy".to_string()
                } else {
                    "sell".to_string()
                },
                slot: tx.slot as i64,
            }));
        }
    }

    Ok(None)
}

/// Helper: Find the User's SOL balance change
fn calculate_sol_change(
    meta: &crate::models::helius_model::TransactionMeta,
    user_wallet: &str,
    account_keys: &[crate::models::helius_model::AccountKey],
) -> u64 {
    let idx = account_keys.iter().position(|k| k.pubkey == user_wallet);
    if let Some(i) = idx {
        let pre = meta.pre_balances.get(i).copied().unwrap_or(0) as i64;
        let post = meta.post_balances.get(i).copied().unwrap_or(0) as i64;
        return (pre - post).abs() as u64;
    }
    0
}

/// Helper: Find the Bonding Curve Account and return its (SOL Balance, Token Balance)
fn find_bonding_curve_reserves(
    meta: &crate::models::helius_model::TransactionMeta,
    mint: &str,
    user_account_index: i64,
    account_keys: &[crate::models::helius_model::AccountKey],
) -> (u64, u64) {
    let empty_vec = vec![];
    let post_token_balances = meta.post_token_balances.as_ref().unwrap_or(&empty_vec);

    // Find the token account for this mint that is NOT the user's account
    if let Some(curve_token_account) = post_token_balances
        .iter()
        .find(|p| p.mint == mint && p.account_index as i64 != user_account_index)
    {
        let real_token_reserves = curve_token_account
            .ui_token_amount
            .amount
            .parse::<u64>()
            .unwrap_or(0);

        // Find SOL balance of the curve owner
        if let Some(owner_address) = &curve_token_account.owner {
            if let Some(owner_idx) = account_keys.iter().position(|k| k.pubkey == *owner_address) {
                let real_sol_reserves = meta.post_balances.get(owner_idx).copied().unwrap_or(0);
                return (real_sol_reserves, real_token_reserves);
            }
        }
    }
    (0, 0)
}

/// Parse token creation from pump.fun
pub fn parse_token_creation(tx: &TransactionResult) -> Result<Option<Token>> {
    // Check if this is a pump.fun program interaction
    let is_pump_fun = tx
        .transaction
        .message
        .instructions
        .iter()
        .any(|ix| ix.program_id == PUMP_FUN_PROGRAM_ID);

    if !is_pump_fun {
        return Ok(None);
    }

    // Look for token mint creation in post_token_balances
    if let Some(meta) = &tx.meta {
        let empty_vec = vec![];
        let post_balances = meta.post_token_balances.as_ref().unwrap_or(&empty_vec);

        // Find new token mints (accounts that appear in post but not pre)
        let pre_balances = meta.pre_token_balances.as_ref().unwrap_or(&empty_vec);

        for post in post_balances {
            // Check if this is a new token (not in pre_balances)
            let is_new = !pre_balances.iter().any(|p| p.mint == post.mint);

            if is_new && post.mint != SOL_MINT {
                let mint_address = post.mint.clone();
                let creator_wallet = tx
                    .transaction
                    .message
                    .account_keys
                    .iter()
                    .find(|k| k.signer)
                    .map(|k| k.pubkey.clone())
                    .unwrap_or_default();

                // Extract metadata from logs
                let (name, symbol, uri) = extract_metadata_from_logs(meta);

                // Find bonding curve address (typically one of the writable accounts)
                let bonding_curve_address =
                    find_bonding_curve_address(&tx.transaction.message.account_keys, &mint_address);

                // Find bonding curve reserves
                let (real_sol_reserves, real_token_reserves) = find_bonding_curve_reserves(
                    meta,
                    &mint_address,
                    -1, // Not user account
                    &tx.transaction.message.account_keys,
                );

                let virtual_sol = real_sol_reserves + VIRTUAL_SOL_OFFSET;
                let virtual_token = real_token_reserves;

                return Ok(Some(Token {
                    mint_address,
                    name,
                    symbol,
                    uri,
                    bonding_curve_address,
                    creator_wallet: Some(creator_wallet),
                    virtual_token_reserves: Decimal::from_u64(virtual_token)
                        .unwrap_or(Decimal::ZERO),
                    virtual_sol_reserves: Decimal::from_u64(virtual_sol).unwrap_or(Decimal::ZERO),
                    real_token_reserves: Decimal::from_u64(real_token_reserves)
                        .unwrap_or(Decimal::ZERO),
                    token_total_supply: Decimal::from_u64(1_000_000_000_000_000)
                        .unwrap_or(Decimal::ZERO), // Standard pump.fun supply
                    market_cap_usd: Decimal::ZERO,
                    bonding_curve_progress: Decimal::ZERO,
                    complete: false,
                    created_at: chrono::Utc::now(),
                    updated_at: None,
                }));
            }
        }
    }

    Ok(None)
}

/// Extract token metadata (name, symbol, uri) from transaction logs
fn extract_metadata_from_logs(
    meta: &crate::models::helius_model::TransactionMeta,
) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(logs) = &meta.log_messages {
        let mut name = None;
        let mut symbol = None;
        let mut uri = None;

        for log in logs {
            // Look for metadata in logs - Pump.fun often logs this info
            // Format examples:
            // "name: TokenName"
            // "symbol: TKN"
            // "uri: https://..."

            if log.contains("name:") || log.contains("Name:") {
                if let Some(pos) = log.find("name:").or_else(|| log.find("Name:")) {
                    let start = pos + 5;
                    let value = log[start..].trim().split(',').next().unwrap_or("").trim();
                    if !value.is_empty() && value.len() < 100 {
                        name = Some(value.to_string());
                    }
                }
            }

            if log.contains("symbol:") || log.contains("Symbol:") {
                if let Some(pos) = log.find("symbol:").or_else(|| log.find("Symbol:")) {
                    let start = pos + 7;
                    let value = log[start..].trim().split(',').next().unwrap_or("").trim();
                    if !value.is_empty() && value.len() < 20 {
                        symbol = Some(value.to_string());
                    }
                }
            }

            if log.contains("uri:") || log.contains("Uri:") || log.contains("metadata:") {
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
                        .split_whitespace()
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

    (None, None, None)
}

/// Find bonding curve address from account keys
/// Pump.fun bonding curve is typically:
/// - A writable PDA (Program Derived Address)
/// - Position 4 or 5 in the accounts array for trades
/// - Contains SOL balance (from pre/post balances)
fn find_bonding_curve_address(
    account_keys: &[crate::models::helius_model::AccountKey],
    mint_address: &str,
) -> Option<String> {
    // Known program addresses to exclude
    const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
    const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
    const RENT_PROGRAM: &str = "SysvarRent111111111111111111111111111111111";
    const EVENT_AUTHORITY: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";

    // Collect potential bonding curve candidates
    let mut candidates = Vec::new();

    for (idx, account) in account_keys.iter().enumerate() {
        // Bonding curve must be writable and not a signer
        if !account.writable || account.signer {
            continue;
        }

        // Skip known program addresses
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

        // Bonding curve is typically at position 4-6 in Pump.fun transactions
        // and is a base58 address (44 chars for standard Solana addresses)
        if idx >= 3 && idx <= 7 && account.pubkey.len() == 44 {
            candidates.push((idx, account.pubkey.clone()));
        }
    }

    // Return the first valid candidate (typically position 4 or 5)
    candidates.first().map(|(_, addr)| addr.clone())
}
