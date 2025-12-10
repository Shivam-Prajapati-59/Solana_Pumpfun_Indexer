use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// Configuration
const MAX_RETRIES: u32 = 5;
const INITIAL_RETRY_DELAY: u64 = 1000;
const PING_INTERVAL: u64 = 30000;

#[derive(Debug)]
pub struct WebSocketClient {
    api_key: String,
    retry_count: u32,
    subscription_id: Option<u64>,
}

impl WebSocketClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            retry_count: 0,
            subscription_id: None,
        }
    }

    pub async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("wss://mainnet.helius-rpc.com/?api-key={}", self.api_key);

        println!("Connecting to WebSocket...");

        let (ws_stream, _) = connect_async(&url).await?;
        let (write, mut read) = ws_stream.split();

        // Wrap write in Arc<Mutex<>> to share between tasks
        let write = Arc::new(Mutex::new(write));

        // Send subscription request for pump.fun program
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "logsSubscribe",
            "params": [
                {
                    "mentions": ["6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"]
                },
                {
                    "commitment": "confirmed"
                }
            ]
        });

        println!(
            "Sending subscription request: {}",
            serde_json::to_string_pretty(&request)?
        );

        write
            .lock()
            .await
            .send(Message::Text(request.to_string().into())) // <-- Added .into()
            .await?;

        self.retry_count = 0;

        // Start ping task
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
                    // <-- Added .into()
                    eprintln!("Failed to send ping");
                    break;
                }
                println!("Ping sent");
            }
        });

        // Handle incoming messages
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Err(e) = self.handle_message(&text).await {
                        eprintln!("Error handling message: {}", e);
                    }
                }
                Ok(Message::Pong(_)) => {
                    println!("Pong received");
                }
                Ok(Message::Close(_)) => {
                    println!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    eprintln!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        ping_task.abort();
        self.reconnect().await
    }

    async fn handle_message(
        &mut self,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message: Value = serde_json::from_str(text)?;

        // Handle subscription confirmation
        if let Some(result) = message.get("result") {
            if let Some(id) = result.as_u64() {
                self.subscription_id = Some(id);
                println!("✅ Successfully subscribed with ID: {}", id);
                return Ok(());
            }
        }

        // Handle actual log data
        if let Some(params) = message.get("params") {
            if let Some(result) = params.get("result") {
                println!(
                    "📥 Received log data: {}",
                    serde_json::to_string_pretty(result)?
                );

                // Extract the transaction signature
                if let Some(signature) = result.get("value").and_then(|v| v.get("signature")) {
                    if let Some(sig_str) = signature.as_str() {
                        println!("🔗 Transaction signature: {}", sig_str);
                    }
                }
            }
        } else {
            println!(
                "📨 Received message: {}",
                serde_json::to_string_pretty(&message)?
            );
        }

        Ok(())
    }

    async fn reconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.retry_count >= MAX_RETRIES {
            eprintln!("Max retry attempts reached. Please check your connection and try again.");
            return Err("Max retries exceeded".into());
        }

        let delay = INITIAL_RETRY_DELAY * 2_u64.pow(self.retry_count);
        println!(
            "🔄 Attempting to reconnect in {} seconds... (Attempt {}/{})",
            delay / 1000,
            self.retry_count + 1,
            MAX_RETRIES
        );

        sleep(Duration::from_millis(delay)).await;
        self.retry_count += 1;

        // Box::pin the recursive call to avoid infinite-sized future
        Box::pin(self.connect()).await
    }
}

/// Public function to run the WebSocket test - called from main.rs
pub async fn run_websocket_test() -> Result<(), Box<dyn std::error::Error>> {
    let api_key =
        std::env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY environment variable must be set");

    let mut client = WebSocketClient::new(api_key);
    client.connect().await; // <-- Added ? to propagate error

    Ok(())
}
