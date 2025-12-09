CREATE EXTENSION IF NOT EXISTS timescaledb;

-- 1. Clean up
DROP TABLE IF EXISTS indexer_stats CASCADE;
DROP TABLE IF EXISTS transactions CASCADE;
DROP TABLE IF EXISTS token_holders CASCADE;
DROP TABLE IF EXISTS trades CASCADE;
DROP TABLE IF EXISTS tokens CASCADE;

-- 2. Tokens Table
CREATE TABLE tokens (
    mint_address TEXT PRIMARY KEY NOT NULL,
    name TEXT,
    symbol TEXT,
    uri TEXT,
    bonding_curve_address TEXT,
    creator_wallet TEXT,
    
    virtual_token_reserves NUMERIC(20,0) DEFAULT 0,
    virtual_sol_reserves NUMERIC(20,0) DEFAULT 0,
    real_token_reserves NUMERIC(20,0) DEFAULT 0,
    token_total_supply NUMERIC(20,0) DEFAULT 0,
    
    market_cap_usd DECIMAL(20, 2) DEFAULT 0,
    bonding_curve_progress DECIMAL(5, 2) DEFAULT 0,

    complete BOOLEAN DEFAULT FALSE,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- 3. Trades Table (The Heavy Hitter)
-- converted to Hypertable for performance
CREATE TABLE trades (
    signature TEXT NOT NULL,
    token_mint TEXT NOT NULL, 
    sol_amount NUMERIC(20,0) NOT NULL,
    token_amount NUMERIC(20,0) NOT NULL,
    is_buy BOOLEAN NOT NULL,
    user_wallet TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,

    virtual_sol_reserves NUMERIC(20,0) NOT NULL,
    virtual_token_reserves NUMERIC(20,0) NOT NULL,
    price_sol DECIMAL(30, 15), -- Derived from sol_amount / token_amount
    price_usd DECIMAL(20, 10),

    track_volume BOOLEAN DEFAULT TRUE,
    ix_name TEXT NOT NULL, -- 'buy' or 'sell'
    slot BIGINT NOT NULL,  -- Crucial for reorg handling
    
    -- Composite Primary Key is required for TimescaleDB Hypertables
    PRIMARY KEY (timestamp, signature) 
);

-- Convert 'trades' to a Hypertable partitioned by time
SELECT create_hypertable('trades', 'timestamp');

-- 4. Token Holders (Upsert Heavy)
CREATE TABLE token_holders (
    token_mint TEXT NOT NULL,
    user_wallet TEXT NOT NULL,
    balance NUMERIC(20,0) NOT NULL DEFAULT 0,
    last_updated_slot BIGINT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    
    PRIMARY KEY (token_mint, user_wallet)
);

-- 5. Raw Transactions (Optional Audit Log)
-- Only store if you truly need raw replay capability.
CREATE TABLE transactions (
    signature TEXT NOT NULL,
    slot BIGINT NOT NULL,
    block_time TIMESTAMPTZ NOT NULL,
    signer TEXT NOT NULL,
    success BOOLEAN NOT NULL,
    instruction_count INTEGER,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    
    PRIMARY KEY (block_time, signature)
);

SELECT create_hypertable('transactions', 'block_time');

-- 6. Optimized Indexes

-- Fast lookups for "Latest Trades for Token X"
-- This combines with the time-partitioning of TimescaleDB
CREATE INDEX idx_trades_mint_time ON trades (token_mint, timestamp DESC);

-- Leaderboard queries (Volume/MarketCap)
CREATE INDEX idx_tokens_market_cap ON tokens (market_cap_usd DESC);
CREATE INDEX idx_tokens_created_at ON tokens (created_at DESC);

-- Holder lookups
CREATE INDEX idx_holders_mint_balance ON token_holders (token_mint, balance DESC);

-- 7. Stats View (Instead of a Table)
-- Use this for your dashboard. It calculates on read.
-- For 100M+ rows, switch this to a Materialized View with periodic refresh.
CREATE VIEW view_collection_stats AS
SELECT 
    COUNT(*) as total_trades,
    (SELECT COUNT(*) FROM tokens) as total_tokens
FROM trades;