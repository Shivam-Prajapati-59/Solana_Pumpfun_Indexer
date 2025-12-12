use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

// ==========================================
// DATABASE MODELS (Match SQL Schema Exactly)
// ==========================================

/// Tokens table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Token {
    pub mint_address: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub uri: Option<String>,
    pub bonding_curve_address: Option<String>,
    pub creator_wallet: Option<String>,

    pub virtual_token_reserves: Decimal,
    pub virtual_sol_reserves: Decimal,
    pub real_token_reserves: Decimal,
    pub token_total_supply: Decimal,

    pub market_cap_usd: Decimal,
    pub bonding_curve_progress: Decimal,
    pub complete: bool,

    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Trades Hypertable - PRIMARY KEY (timestamp, signature)
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Trade {
    pub signature: String,
    pub token_mint: String,
    pub sol_amount: Decimal,
    pub token_amount: Decimal,
    pub is_buy: bool,
    pub user_wallet: String,
    pub timestamp: DateTime<Utc>,

    pub virtual_sol_reserves: Decimal,
    pub virtual_token_reserves: Decimal,
    pub price_sol: Option<Decimal>,
    pub price_usd: Option<Decimal>,

    pub track_volume: bool,
    pub ix_name: String, // 'buy' or 'sell'
    pub slot: i64,
}

/// Token Holders - PRIMARY KEY (token_mint, user_wallet)
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TokenHolder {
    pub token_mint: String,
    pub user_wallet: String,
    pub balance: Decimal,
    pub last_updated_slot: i64,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Transactions table (optional audit log) - PRIMARY KEY (block_time, signature)
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Transaction {
    pub signature: String,
    pub slot: i64,
    pub block_time: DateTime<Utc>,
    pub signer: String,
    pub success: bool,
    pub instruction_count: Option<i32>,
    pub created_at: Option<DateTime<Utc>>,
}
