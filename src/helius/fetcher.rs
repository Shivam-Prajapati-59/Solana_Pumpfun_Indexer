use crate::models::helius_model::TransactionResult;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

pub async fn fetch_and_process_transaction(api_key: String, signature: String) -> Result<()> {
    sleep(Duration::from_secs(2)).await;

    println!("🔍 [Background] Fetching transaction: {}", signature);

    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", api_key);
    let client = reqwest::Client::new();

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [
            &signature, // Pass reference to avoid cloning
            {
                "encoding": "jsonParsed", // CRITICAL: Changed 'json' to 'jsonParsed' for easier reading
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    // STEP 2: The Retry Loop
    let max_retries = 5;
    let mut attempt = 0;

    loop {
        attempt += 1;

        let response = client.post(&rpc_url).json(&request).send().await;

        match response {
            Ok(resp) => {
                // Handle Rate Limits (HTTP 429)
                if resp.status() == 429 {
                    if attempt >= max_retries {
                        eprintln!("❌ [Give Up] Rate limited too many times: {}", signature);
                        return Ok(());
                    }
                    eprintln!("⏳ Rate limited on attempt {}. Cooling down...", attempt);
                    sleep(Duration::from_secs(attempt * 2)).await; // Exponential backoff
                    continue;
                }

                // Parse the response body
                let response_text = resp.text().await.context("Failed to get text")?;
                let response_json: Value =
                    serde_json::from_str(&response_text).context("Failed to parse JSON")?;

                // Check for RPC Errors
                if let Some(error) = response_json.get("error") {
                    eprintln!("RPC Error for {}: {}", signature, error);
                    return Ok(()); // Stop trying if it's a hard error
                }

                // Check for Result
                if let Some(result) = response_json.get("result") {
                    // CASE A: Transaction not found yet (null)
                    if result.is_null() {
                        if attempt >= max_retries {
                            println!(
                                "⚠️ Transaction {} never appeared after {} retries",
                                signature, max_retries
                            );
                            return Ok(());
                        }
                        println!(
                            "... Tx not found yet (Attempt {}/{}), waiting...",
                            attempt, max_retries
                        );
                        sleep(Duration::from_secs(2)).await;
                        continue;
                    }

                    // CASE B: Success! Parse and Process
                    match serde_json::from_value::<TransactionResult>(result.clone()) {
                        Ok(tx_result) => {
                            println!("✅ [Background] Successfully parsed: {}", signature);
                            process_transaction(tx_result).await?;
                            return Ok(()); // Exit the loop and function
                        }
                        Err(e) => {
                            eprintln!("❌ Failed to parse struct for {}: {}", signature, e);
                            // Optional: Print raw json to debug structure mismatches
                            // println!("Raw JSON: {}", result);
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Network error on attempt {}: {}", attempt, e);
                sleep(Duration::from_secs(1)).await;
            }
        }

        if attempt >= max_retries {
            break;
        }
    }

    Ok(())
}

// ---------------------------------------------------------
// 2. The Processor (Logic & Display)
// ---------------------------------------------------------
async fn process_transaction(tx: TransactionResult) -> Result<()> {
    println!("\n=== Transaction Details ===");
    println!("Slot: {}", tx.slot);
    // Handle Option<i64> safely for printing
    if let Some(t) = tx.block_time {
        println!("Block Time: {}", t);
    }

    // Safety check: Ensure signatures array is not empty
    if let Some(sig) = tx.transaction.signatures.first() {
        println!("Signature: {}", sig);
    }

    // Check if transaction succeeded
    if let Some(meta) = &tx.meta {
        if let Some(err) = &meta.err {
            if !err.is_null() {
                println!("Status: ❌ Failed");
                return Ok(());
            }
        }
        println!("Status: ✅ Success");
        println!("Fee: {} lamports", meta.fee);

        if let Some(compute) = meta.compute_units_consumed {
            println!("Compute Units: {}", compute);
        }

        // Print logs
        if let Some(logs) = &meta.log_messages {
            println!("\n📜 Logs (First 5):");
            for log in logs.iter().take(5) {
                println!("   {}", log);
            }
        }

        // Token balances
        if let Some(post_balances) = &meta.post_token_balances {
            println!("\n💰 Token Balances Changes:");
            for balance in post_balances {
                println!(
                    "   Mint: {} | Amount: {}",
                    balance.mint,
                    balance
                        .ui_token_amount
                        .ui_amount_string
                        .clone()
                        .unwrap_or_default()
                );
            }
        }
    }

    // Print instructions
    // Note: ensure your TransactionMessage struct has 'instructions' field
    println!(
        "\n📋 Instructions ({}):",
        tx.transaction.message.instructions.len()
    );

    for (i, ix) in tx.transaction.message.instructions.iter().enumerate() {
        println!("   {}. Program: {}", i + 1, ix.program_id);

        // If it's a Pump.fun trade, the data is usually in 'data', not 'parsed'
        // unless you have a specific parser for it.
        if let Some(data) = &ix.data {
            println!(
                "      Raw Data: {}...",
                &data.chars().take(20).collect::<String>()
            );
        }
    }

    println!("=========================\n");

    // TODO: DB Insert goes here
    Ok(())
}
