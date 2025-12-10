use serde::{Deserialize, Serialize};
use serde_json::Value;

// ------------------------------------------
// 1. TOP LEVEL RESPONSE
// ------------------------------------------
#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionResult {
    pub slot: u64,
    #[serde(rename = "blockTime")]
    pub block_time: Option<i64>,
    pub transaction: TransactionData,
    pub meta: Option<TransactionMeta>,
}

// ------------------------------------------
// 2. TRANSACTION DATA
// ------------------------------------------
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

// ------------------------------------------
// 3. INSTRUCTIONS (The Hard Part)
// ------------------------------------------
#[derive(Debug, Deserialize, Serialize)]
pub struct Instruction {
    #[serde(rename = "programId")]
    pub program_id: String,
    pub accounts: Option<Vec<String>>,
    pub data: Option<String>,
    pub parsed: Option<Value>,
    pub program: Option<String>,
}

// ------------------------------------------
// 4. METADATA (Logs & Balances)
// ------------------------------------------
#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionMeta {
    pub err: Option<Value>,
    pub fee: u64,
    #[serde(rename = "preBalances")]
    pub pre_balances: Vec<u64>,
    #[serde(rename = "postBalances")]
    pub post_balances: Vec<u64>,
    #[serde(rename = "computeUnitsConsumed")]
    pub compute_units_consumed: Option<u64>,

    #[serde(rename = "preTokenBalances")]
    pub pre_token_balances: Option<Vec<TokenBalance>>,
    #[serde(rename = "postTokenBalances")]
    pub post_token_balances: Option<Vec<TokenBalance>>,
    #[serde(rename = "logMessages")]
    pub log_messages: Option<Vec<String>>,
    #[serde(rename = "innerInstructions")]
    pub inner_instructions: Option<Vec<InnerInstructionWrapper>>,
}

// ------------------------------------------
// 5. HELPER STRUCTS
// ------------------------------------------
#[derive(Debug, Deserialize, Serialize)]
pub struct TokenBalance {
    #[serde(rename = "accountIndex")]
    pub account_index: u32,
    pub mint: String,
    #[serde(rename = "uiTokenAmount")]
    pub ui_token_amount: UiTokenAmount,
    pub owner: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UiTokenAmount {
    #[serde(rename = "uiAmount")]
    pub ui_amount: Option<f64>,
    pub decimals: u8,
    pub amount: String,
    #[serde(rename = "uiAmountString")]
    pub ui_amount_string: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InnerInstructionWrapper {
    pub index: u32,
    pub instructions: Vec<Instruction>,
}
