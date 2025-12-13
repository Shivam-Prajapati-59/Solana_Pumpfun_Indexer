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

pub fn parse_token_creation(_tx: &TransactionResult) -> Result<Option<Token>> {
    Ok(None)
}
