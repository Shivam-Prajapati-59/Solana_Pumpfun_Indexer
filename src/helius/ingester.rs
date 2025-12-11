use crate::redis::redis_cleint::RedisClient;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const PING_INTERVAL: u64 = 30_000;
const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const REDIS_CHANNEL: &str = "solana:transactions";

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LogMessage {
    Update { params: UpdateParams },
    Confirmation { result: u64, id: u64 },
}

#[derive(Debug, Deserialize)]
struct UpdateParams {
    result: UpdateResult,
}

#[derive(Debug, Deserialize)]
struct UpdateResult {
    value: TransactionInfo,
}

// Minimal info to send over Redis
#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionInfo {
    pub signature: String,
    pub err: Option<serde_json::Value>,
}

pub struct WebSocketClient {
    api_key: String,
    redis_client: RedisClient,
}

impl WebSocketClient {
    pub fn new(api_key: String, redis_client: RedisClient) -> Self {
        Self {
            api_key,
            redis_client,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        let url = format!("wss://mainnet.helius-rpc.com/?api-key={}", self.api_key);
        println!("🔌 Connecting to Helius WebSocket...");

        let (ws_stream, _) = connect_async(&url).await?;
        println!("✅ Connected to WebSocket!");

        let (mut write, mut read) = ws_stream.split();

        // 1. Send subscription
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "logsSubscribe",
            "params": [
                { "mentions": [PUMP_FUN_PROGRAM_ID] },
                { "commitment": "confirmed" }
            ]
        });
        write
            .send(Message::Text(request.to_string().into()))
            .await?;

        // 2. Background Ping Task
        let ping_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(PING_INTERVAL));
            loop {
                interval.tick().await;
                if write.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        });

        // 3. Process Messages
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(LogMessage::Update { params }) =
                        serde_json::from_str::<LogMessage>(&text)
                    {
                        let tx_info = params.result.value;

                        // Ignore failed transactions
                        if tx_info.err.is_some() {
                            continue;
                        }

                        println!("📥 Detected: {}", tx_info.signature);

                        // PUBLISH to Redis
                        if let Err(e) = self.redis_client.publish(REDIS_CHANNEL, &tx_info).await {
                            eprintln!("❌ Publish failed: {}", e);
                        } else {
                            println!("📡 Published to channel: {}", REDIS_CHANNEL);
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }

        ping_task.abort();
        Err(anyhow::anyhow!("Connection closed"))
    }
}

pub async fn run_ingester() -> Result<()> {
    let api_key = std::env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY missing");
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let redis_client = RedisClient::new(&redis_url).await?;
    let mut client = WebSocketClient::new(api_key, redis_client);

    loop {
        if let Err(e) = client.connect().await {
            eprintln!("⚠️ Disconnected: {}. Retrying in 5s...", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
