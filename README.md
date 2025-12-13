# Pump.fun Indexer

Real-time indexer for Pump.fun trades on Solana using Helius WebSocket API, Redis, and PostgreSQL/.

## Architecture

```
Helius WebSocket → Ingester → Redis → Worker → Parser → PostgreSQL
```

- **Ingester**: Listens to Helius WebSocket for transaction signatures
- **Redis**: Message queue for decoupling components
- **Worker**: Fetches full transaction details via RPC
- **Parser**: Extracts trade data from transactions
- **Database**: Stores trades, tokens, and analytics

## Prerequisites

- Rust (latest stable)
- PostgreSQL 14+ with extension
- Redis
- Helius API key

## Setup

### 1. Install Dependencies

```bash
# PostgreSQL &
# Ubuntu/Debian:
sudo apt-get install postgresql postgresql-contrib
sudo apt-get install -postgresql-14

# macOS:
brew install postgresql

# Redis
# Ubuntu/Debian:
sudo apt-get install redis-server

# macOS:
brew install redis
```

### 2. Configure Environment

Copy the example environment file:

```bash
cp .env.example .env
```

Edit `.env`:

```env
# Helius
HELIUS_API_KEY=your-actual-helius-api-key

# Redis
REDIS_URL=redis://127.0.0.1:6379

# PostgreSQL/
DATABASE_URL=postgresql://username:password@localhost:5432/pump_indexer
```

### 3. Create Database

```bash
# Create database
createdb pump_indexer

# Or via psql:
psql -U postgres -c "CREATE DATABASE pump_indexer;"

# Enable  extension
psql -U postgres -d pump_indexer -c "CREATE EXTENSION IF NOT EXISTS ;"
```

### 4. Run Migrations

```bash
# Install sqlx-cli
cargo install sqlx-cli --no-default-features --features postgres,native-tls

# Run migrations
sqlx migrate run
```

### 5. Start Services

Make sure Redis and PostgreSQL are running:

```bash
# Start Redis
redis-server

# Check PostgreSQL
pg_ctl status
```

## Running the Indexer

You need to run **two separate processes**:

### Terminal 1: Ingester (WebSocket → Redis)

```bash
cargo run --bin indexer
```

This listens to Helius WebSocket and publishes transaction signatures to Redis.

### Terminal 2: Worker (Redis → Database)

```bash
cargo run --bin worker
```

This subscribes to Redis, fetches full transactions, parses them, and saves to PostgreSQL.

## Expected Output

**Ingester Terminal:**

```
🚀 Starting Pump.fun Indexer - WebSocket Ingester
===================================================

📥 Detected: <signature>
📡 Published to channel: solana:transactions
```

**Worker Terminal:**

```
🎧 Starting Pump.fun Indexer - Worker
======================================

✅ Database connected
🎧 Worker started. Listening on: solana:transactions
⚡ Event Received: {"signature":"...","err":null}
🔍 Fetching details for: <signature>
📊 Processing transaction: <signature>
💰 Trade detected: BUY 1000000 tokens for 0.05 SOL
✅ Trade saved to DB: <signature>
```

## Database Schema

### Tables

- **tokens**: Token metadata (mint, symbol, bonding curve, reserves)
- **trades**: All buy/sell transactions (hypertable for time-series)
- **token_holders**: Current token balances per wallet
- **transactions**: Audit log of all processed transactions

### Key Queries

```sql
-- Recent trades for a token
SELECT * FROM trades
WHERE token_mint = '<mint_address>'
ORDER BY timestamp DESC
LIMIT 100;

-- 24h volume
SELECT SUM(sol_amount) as volume_24h
FROM trades
WHERE timestamp > NOW() - INTERVAL '24 hours';

-- Top holders
SELECT * FROM token_holders
WHERE token_mint = '<mint_address>'
ORDER BY balance DESC
LIMIT 10;
```

## Development

### Building

```bash
# Check compilation
cargo check

# Build release
cargo build --release

# Run tests
cargo test
```

## Troubleshooting

### "relation 'tokens' does not exist"

Run migrations:

```bash
sqlx migrate run
```

### Worker can't connect to database

Check your `DATABASE_URL` in `.env` and ensure PostgreSQL is running:

```bash
psql $DATABASE_URL -c "SELECT 1;"
```

### Ingester can't connect to WebSocket

Verify your `HELIUS_API_KEY` is valid and has WebSocket access.

### No events appearing

Make sure both ingester AND worker are running in separate terminals.

## License

MIT
