# 🚀 Pump.fun Indexer

A high-performance, real-time indexer for Pump.fun token trades on Solana.

## 📊 Architecture

```
┌─────────────────────┐
│  Helius WebSocket   │  ← Real-time transaction stream
│   (logsSubscribe)   │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│     Ingester        │  ← Filters Pump.fun transactions
│  (src/main.rs)      │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│   Redis Pub/Sub     │  ← Message queue
│  (solana:txs)       │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│      Worker         │  ← Fetches full transaction data
│ (src/bin/worker.rs) │     + Extracts metadata from logs
└──────────┬──────────┘     + Parses trades & token info
           │                + Calculates market metrics
           ▼
┌─────────────────────┐
│  PostgreSQL + TSB   │  ← Stores trades, tokens, holders
│  ┌─────────────┐    │
│  │   tokens    │    │  ← Metadata, reserves, market cap
│  ├─────────────┤    │
│  │   trades    │    │  ← Buy/sell events (hypertable)
│  ├─────────────┤    │
│  │token_holders│    │  ← Wallet balances
│  └─────────────┘    │
└─────────────────────┘
           │
           ▼
┌─────────────────────┐
│   Pyth Network      │  ← Real-time SOL/USD price
│  (Hermes API)       │     (30s cache)
└─────────────────────┘
```

## ✨ Features

- ✅ Real-time WebSocket event streaming
- ✅ Complete metadata extraction (name, symbol, URI)
- ✅ Bonding curve address detection
- ✅ Live SOL/USD pricing via Pyth Network
- ✅ Market cap & bonding curve progress tracking
- ✅ Token holder balance updates
- ✅ TimescaleDB hypertables for efficient queries
- ✅ Auto-reconnection & error handling

## 📋 Prerequisites

- **Rust** 1.70+
- **PostgreSQL** 14+ with TimescaleDB
- **Redis** 6+
- **Helius API Key** ([Get one here](https://helius.dev))

## 🛠️ Quick Start

### 1. Install Dependencies

```bash
# Ubuntu/Debian
sudo apt install postgresql postgresql-14-timescaledb redis-server

# macOS
brew install postgresql@14 timescaledb redis
```

### 2. Setup Database

```bash
# Create database
createdb pump_indexer

# Enable TimescaleDB
psql pump_indexer -c "CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;"

# Run migrations
psql pump_indexer -f migrations/20240101000000_init_schema.sql
```

### 3. Configure Environment

```bash
cat > .env << EOF
DATABASE_URL=postgresql://postgres:password@localhost/pump_indexer
REDIS_URL=redis://127.0.0.1:6379
HELIUS_API_KEY=your_helius_api_key_here
EOF
```

### 4. Build & Run

```bash
# Build release binaries
cargo build --release

# Terminal 1: Start Ingester
cargo run --release

# Terminal 2: Start Worker
cargo run --release --bin worker
```

## 📊 Database Schema

### Tables

| Table           | Description                          | Records    |
| --------------- | ------------------------------------ | ---------- |
| `tokens`        | Token metadata, reserves, market cap | Per token  |
| `trades`        | All buy/sell transactions            | Per trade  |
| `token_holders` | Real-time wallet balances            | Per holder |

### Sample Queries

```sql
-- Top tokens by market cap
SELECT mint_address, symbol, market_cap_usd, bonding_curve_progress
FROM tokens
ORDER BY market_cap_usd DESC
LIMIT 10;

-- Recent trades
SELECT timestamp, is_buy, token_amount, sol_amount, price_usd
FROM trades
WHERE token_mint = 'YOUR_MINT'
ORDER BY timestamp DESC
LIMIT 20;

-- Top holders
SELECT user_wallet, balance
FROM token_holders
WHERE token_mint = 'YOUR_MINT'
ORDER BY balance DESC
LIMIT 50;

-- 24h volume
SELECT SUM(sol_amount) / 1e9 as volume_sol
FROM trades
WHERE timestamp > NOW() - INTERVAL '24 hours';
```

## 🔍 What Gets Indexed

### Token Creation

- Mint address & creator wallet
- Name, symbol, URI (from logs)
- Bonding curve address (PDA detection)
- Initial reserves & market cap

### Every Trade

- Buy/sell detection
- Token & SOL amounts
- Real-time USD pricing
- Virtual reserves snapshot
- User wallet balance updates

### Calculated Metrics

- Market cap (USD)
- Bonding curve progress (0-100%)
- Price per token (SOL & USD)
- Holder distribution

## 📈 Monitoring

The indexer outputs detailed logs:

```
🪙 New token created: EPjF...Dt1v
✅ Token saved to DB (Market Cap: $1,234.56)
💰 Trade detected: BUY 1000000 tokens for 0.5 SOL ($49.25)
✅ Trade saved to DB
✅ Token saved/updated (Market Cap: $1,350.00, Progress: 45.2%)
✅ Token holder updated: ABC...xyz (balance: 1000000)
```

## 🐛 Troubleshooting

**WebSocket disconnects:**

- Auto-reconnects every 5 seconds
- Check Helius API key validity

**Missing trades:**

- Ensure both ingester AND worker are running
- Verify Redis connectivity: `redis-cli ping`

## 📄 License

MIT
