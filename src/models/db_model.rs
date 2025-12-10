use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

// ==========================================
// 1. DATABASE MODELS (Postgres / TimescaleDB)
// ==========================================

/// Represents a Token metadata entry in the 'tokens' table.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Token {
    pub mint_address: String,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub bonding_curve_address: String,
    pub creator_wallet: String,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub token_total_supply: u64,
    pub market_cap_usd: u64,
    pub bonding_curve_progress: u64,
    pub complete: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Represents a Trade event in the 'trades' Hypertable.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Trade {
    pub signature: String,
    pub token_mint: String,
    pub sol_amount: u64,
    pub token_amount: u64,
    pub is_buy: bool,
    pub user_wallet: String,
    pub timestamp: DateTime<Utc>,
    pub virtual_sol_reserves: u64,
    pub virtual_token_reserves: u64,
    pub price_sol: Option<u64>,
    pub price_usd: Option<u64>,
    pub track_volume: bool,
    pub ix_name: String, // 'buy' or 'sell'
    pub slot: i64,       // Used for reorg handling
}

/// Represents the current state of a holder in 'token_holders' table.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TokenHolder {
    pub token_mint: String,
    pub user_wallet: String,
    pub balance: u64,
    pub last_updated_slot: i64,
    pub updated_at: DateTime<Utc>,
}
