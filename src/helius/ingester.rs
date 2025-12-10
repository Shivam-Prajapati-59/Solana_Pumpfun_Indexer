use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::helius::fetcher;

// Configuration
const MAX_RETRIES: u32 = 5;
const INITIAL_RETRY_DELAY: u64 = 1000;
const PING_INTERVAL: u64 = 30000;
const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

// ---------------------------------------------------------
// Local WebSocket Message Structs
// ---------------------------------------------------------
#[derive(Debug, serde::Deserialize)]
struct LogSubscriptionResult {
    #[serde(rename = "value")]
    pub value: LogValue,
    pub context: Option<LogContext>,
}

#[derive(Debug, serde::Deserialize)]
struct LogValue {
    pub signature: String,
    pub err: Option<Value>,
    pub logs: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct LogContext {
    pub slot: u64,
}

// ---------------------------------------------------------
// The WebSocket Client
// ---------------------------------------------------------
#[derive(Debug)]
pub struct WebSocketClient {
    api_key: String,
    subscription_id: Option<u64>,
}

impl WebSocketClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            subscription_id: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("wss://mainnet.helius-rpc.com/?api-key={}", self.api_key);
        println!("🔌 Connecting to Helius WebSocket...");

        let (ws_stream, _) = connect_async(&url).await?;
        let (write, mut read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));

        // Send subscription request
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "logsSubscribe",
            "params": [
                {
                    "mentions": [PUMP_FUN_PROGRAM_ID] // Listening to Pump.fun
                },
                {
                    "commitment": "confirmed"
                }
            ]
        });

        println!("📤 Sending subscription request...");
        write
            .lock()
            .await
            .send(Message::Text(request.to_string().into()))
            .await?;

        // Start Ping Task (Keep-Alive)
        let write_for_ping = Arc::clone(&write);
        let ping_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(PING_INTERVAL));
            loop {
                interval.tick().await;
                let mut write_guard = write_for_ping.lock().await;
                if write_guard
                    .send(Message::Ping(vec![].into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Listen for Messages
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Err(e) = self.handle_message(&text).await {
                        eprintln!("⚠️ Message handler error: {}", e);
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }

        ping_task.abort();
        Err("WebSocket connection closed".into())
    }

    async fn handle_message(
        &mut self,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message: Value = serde_json::from_str(text)?;

        // 1. Handle Subscription Confirmation
        if let Some(result) = message.get("result") {
            if let Some(id) = result.as_u64() {
                self.subscription_id = Some(id);
                println!("✅ Subscribed! Subscription ID: {}", id);
                return Ok(());
            }
        }

        // 2. Handle Log Notifications
        if let Some(params) = message.get("params") {
            if let Some(result) = params.get("result") {
                if let Ok(log_result) =
                    serde_json::from_value::<LogSubscriptionResult>(result.clone())
                {
                    // Filter: Skip failed transactions
                    if log_result.value.err.is_some() {
                        return Ok(());
                    }

                    // Prepare data for the background task
                    let signature = log_result.value.signature.clone();
                    let api_key = self.api_key.clone();

                    println!("📥 Detected Tx: {}", signature);

                    // 🔥 CRITICAL: Spawn the background task using your Fetcher
                    tokio::spawn(async move {
                        // Call the function from your fetcher.rs file
                        if let Err(e) =
                            fetcher::fetch_and_process_transaction(api_key, signature).await
                        {
                            eprintln!("❌ Fetcher error: {}", e);
                        }
                    });
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------
// The Runner Loop (Retry Logic)
// ---------------------------------------------------------
pub async fn run_websocket_test() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY must be set");
    let mut client = WebSocketClient::new(api_key);
    let mut retry_count = 0;

    loop {
        println!("🚀 Starting WebSocket (Attempt {})", retry_count + 1);

        match client.connect().await {
            Ok(_) => retry_count = 0, // Reset retries on clean exit (rare)
            Err(e) => {
                eprintln!("❌ Connection Lost: {}", e);
                retry_count += 1;

                if retry_count >= MAX_RETRIES {
                    eprintln!("💀 Max retries reached. Exiting.");
                    return Err("Max retries exceeded".into());
                }

                // Exponential Backoff
                let delay = INITIAL_RETRY_DELAY * 2_u64.pow(retry_count - 1);
                println!("⏳ Reconnecting in {}s...", delay / 1000);
                sleep(Duration::from_millis(delay)).await;
            }
        }
    }
}
