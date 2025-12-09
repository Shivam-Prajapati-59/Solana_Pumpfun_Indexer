use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use sqlx::types::BigDecimal;

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
    pub virtual_token_reserves: BigDecimal,
    pub virtual_sol_reserves: BigDecimal,
    pub real_token_reserves: BigDecimal,
    pub token_total_supply: BigDecimal,
    pub market_cap_usd: BigDecimal,
    pub bonding_curve_progress: BigDecimal,
    pub complete: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Represents a Trade event in the 'trades' Hypertable.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Trade {
    pub signature: String,
    pub token_mint: String,
    pub sol_amount: BigDecimal,
    pub token_amount: BigDecimal,
    pub is_buy: bool,
    pub user_wallet: String,
    pub timestamp: DateTime<Utc>,
    pub virtual_sol_reserves: BigDecimal,
    pub virtual_token_reserves: BigDecimal,
    pub price_sol: Option<BigDecimal>,
    pub price_usd: Option<BigDecimal>,
    pub track_volume: bool,
    pub ix_name: String, // 'buy' or 'sell'
    pub slot: i64,       // Used for reorg handling
}

/// Represents the current state of a holder in 'token_holders' table.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TokenHolder {
    pub token_mint: String,
    pub user_wallet: String,
    pub balance: BigDecimal,
    pub last_updated_slot: i64,
    pub updated_at: DateTime<Utc>,
}
