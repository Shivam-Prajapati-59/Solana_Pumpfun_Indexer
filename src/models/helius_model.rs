use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use sqlx::types::BigDecimal;

#[derive(Debug, Deserialize, Serialize)]
pub struct HeliusResponse {
    pub jsonrpc: String,
    pub result: Option<TransactionResult>,
    pub id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionResult {
    pub slot: u64,
    #[serde(rename = "blockTime")]
    pub block_time: Option<i64>,
    pub transaction: TransactionData,
    pub meta: Option<TransactionMeta>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionData {
    pub signatures: Vec<String>,
    pub message: TransactionMessage,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionMessage {
    #[serde(rename = "accountKeys")]
    pub account_keys: Vec<AccountKey>,
    pub instructions: Vec<Instruction>,
    #[serde(rename = "recentBlockhash")]
    pub recent_blockhash: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AccountKey {
    pub pubkey: String,
    pub signer: bool,
    pub writable: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionMeta {
    pub err: Option<serde_json::Value>,
    pub fee: u64,
    #[serde(rename = "preBalances")]
    pub pre_balances: Vec<u64>,
    #[serde(rename = "postBalances")]
    pub post_balances: Vec<u64>,
    #[serde(rename = "preTokenBalances")]
    pub pre_token_balances: Option<Vec<TokenBalance>>,
    #[serde(rename = "postTokenBalances")]
    pub post_token_balances: Option<Vec<TokenBalance>>,
    #[serde(rename = "logMessages")]
    pub log_messages: Option<Vec<String>>,
    #[serde(rename = "innerInstructions")]
    pub inner_instructions: Option<Vec<InnerInstructionWrapper>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokenBalance {
    #[serde(rename = "accountIndex")]
    pub account_index: usize,
    pub mint: String,
    #[serde(rename = "uiTokenAmount")]
    pub ui_token_amount: UiTokenAmount,
    pub owner: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UiTokenAmount {
    pub amount: String,
    pub decimals: u8,
    #[serde(rename = "uiAmount")]
    pub ui_amount: Option<f64>,
    #[serde(rename = "uiAmountString")]
    pub ui_amount_string: String,
}

/// Represents an Instruction which might be Parsed (JSON) or Raw (Base58)
#[derive(Debug, Deserialize, Serialize)]
pub struct Instruction {
    #[serde(rename = "programId")]
    pub program_id: String,
    pub accounts: Option<Vec<String>>,
    pub data: Option<String>, // Base58 data if not parsed

    // Sometimes Helius returns a 'parsed' field if it recognizes the program (e.g. SPL Token)
    pub parsed: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InnerInstructionWrapper {
    pub index: u32,
    pub instructions: Vec<Instruction>,
}
