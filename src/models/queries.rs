use super::{Token, TokenHolder, Trade, Transaction};
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};

// ==========================================
// TOKEN OPERATIONS
// ==========================================

/// Insert or update token metadata
pub async fn upsert_token(pool: &PgPool, token: &Token) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO tokens (
            mint_address, name, symbol, uri, bonding_curve_address, creator_wallet,
            virtual_token_reserves, virtual_sol_reserves, real_token_reserves,
            token_total_supply, market_cap_usd, bonding_curve_progress, complete
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        ON CONFLICT (mint_address) 
        DO UPDATE SET
            name = EXCLUDED.name,
            symbol = EXCLUDED.symbol,
            uri = EXCLUDED.uri,
            virtual_token_reserves = EXCLUDED.virtual_token_reserves,
            virtual_sol_reserves = EXCLUDED.virtual_sol_reserves,
            real_token_reserves = EXCLUDED.real_token_reserves,
            token_total_supply = EXCLUDED.token_total_supply,
            market_cap_usd = EXCLUDED.market_cap_usd,
            bonding_curve_progress = EXCLUDED.bonding_curve_progress,
            complete = EXCLUDED.complete,
            updated_at = NOW()
        "#,
    )
    .bind(&token.mint_address)
    .bind(&token.name)
    .bind(&token.symbol)
    .bind(&token.uri)
    .bind(&token.bonding_curve_address)
    .bind(&token.creator_wallet)
    .bind(&token.virtual_token_reserves)
    .bind(&token.virtual_sol_reserves)
    .bind(&token.real_token_reserves)
    .bind(&token.token_total_supply)
    .bind(&token.market_cap_usd)
    .bind(&token.bonding_curve_progress)
    .bind(token.complete)
    .execute(pool)
    .await
    .context("Failed to upsert token")?;

    Ok(())
}

/// Get token by mint address
pub async fn get_token(pool: &PgPool, mint_address: &str) -> Result<Option<Token>> {
    let token = sqlx::query_as::<_, Token>(
        r#"
        SELECT 
            mint_address,
            name,
            symbol,
            uri,
            bonding_curve_address,
            creator_wallet,
            virtual_token_reserves,
            virtual_sol_reserves,
            real_token_reserves,
            token_total_supply,
            market_cap_usd,
            bonding_curve_progress,
            complete,
            created_at,
            updated_at
        FROM tokens 
        WHERE mint_address = $1
        "#,
    )
    .bind(mint_address)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch token")?;

    Ok(token)
}

// ==========================================
// TRADE OPERATIONS
// ==========================================

/// Insert a new trade (no upsert, trades are immutable)
pub async fn insert_trade(pool: &PgPool, trade: &Trade) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO trades (
            signature, token_mint, sol_amount, token_amount, is_buy,
            user_wallet, timestamp, virtual_sol_reserves, virtual_token_reserves,
            price_sol, price_usd, track_volume, ix_name, slot
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        ON CONFLICT (timestamp, signature) DO NOTHING
        "#,
    )
    .bind(&trade.signature)
    .bind(&trade.token_mint)
    .bind(&trade.sol_amount)
    .bind(&trade.token_amount)
    .bind(trade.is_buy)
    .bind(&trade.user_wallet)
    .bind(trade.timestamp)
    .bind(&trade.virtual_sol_reserves)
    .bind(&trade.virtual_token_reserves)
    .bind(&trade.price_sol)
    .bind(&trade.price_usd)
    .bind(trade.track_volume)
    .bind(&trade.ix_name)
    .bind(trade.slot)
    .execute(pool)
    .await
    .context("Failed to insert trade")?;

    Ok(())
}

/// Batch insert trades for efficiency
pub async fn batch_insert_trades(pool: &PgPool, trades: &[Trade]) -> Result<()> {
    if trades.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;

    for trade in trades {
        sqlx::query(
            r#"
            INSERT INTO trades (
                signature, token_mint, sol_amount, token_amount, is_buy,
                user_wallet, timestamp, virtual_sol_reserves, virtual_token_reserves,
                price_sol, price_usd, track_volume, ix_name, slot
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (timestamp, signature) DO NOTHING
            "#,
        )
        .bind(&trade.signature)
        .bind(&trade.token_mint)
        .bind(&trade.sol_amount)
        .bind(&trade.token_amount)
        .bind(trade.is_buy)
        .bind(&trade.user_wallet)
        .bind(trade.timestamp)
        .bind(&trade.virtual_sol_reserves)
        .bind(&trade.virtual_token_reserves)
        .bind(&trade.price_sol)
        .bind(&trade.price_usd)
        .bind(trade.track_volume)
        .bind(&trade.ix_name)
        .bind(trade.slot)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await.context("Failed to commit trade batch")?;
    Ok(())
}

/// Get recent trades for a token
pub async fn get_recent_trades(
    pool: &PgPool,
    mint_address: &str,
    limit: i64,
) -> Result<Vec<Trade>> {
    let trades = sqlx::query_as::<_, Trade>(
        r#"
        SELECT 
            signature,
            token_mint,
            sol_amount,
            token_amount,
            is_buy,
            user_wallet,
            timestamp,
            virtual_sol_reserves,
            virtual_token_reserves,
            price_sol,
            price_usd,
            track_volume,
            ix_name,
            slot
        FROM trades 
        WHERE token_mint = $1 
        ORDER BY timestamp DESC 
        LIMIT $2
        "#,
    )
    .bind(mint_address)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to fetch recent trades")?;

    Ok(trades)
}

// ==========================================
// TOKEN HOLDER OPERATIONS
// ==========================================

/// Upsert token holder balance
pub async fn upsert_token_holder(pool: &PgPool, holder: &TokenHolder) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO token_holders (
            token_mint, user_wallet, balance, last_updated_slot
        ) VALUES ($1, $2, $3, $4)
        ON CONFLICT (token_mint, user_wallet)
        DO UPDATE SET
            balance = EXCLUDED.balance,
            last_updated_slot = EXCLUDED.last_updated_slot,
            updated_at = NOW()
        WHERE token_holders.last_updated_slot < EXCLUDED.last_updated_slot
        "#,
    )
    .bind(&holder.token_mint)
    .bind(&holder.user_wallet)
    .bind(&holder.balance)
    .bind(holder.last_updated_slot)
    .execute(pool)
    .await
    .context("Failed to upsert token holder")?;

    Ok(())
}

/// Get top holders for a token
pub async fn get_top_holders(
    pool: &PgPool,
    mint_address: &str,
    limit: i64,
) -> Result<Vec<TokenHolder>> {
    let holders = sqlx::query_as::<_, TokenHolder>(
        r#"
        SELECT 
            token_mint,
            user_wallet,
            balance,
            last_updated_slot,
            updated_at
        FROM token_holders 
        WHERE token_mint = $1 
        ORDER BY balance DESC 
        LIMIT $2
        "#,
    )
    .bind(mint_address)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("Failed to fetch top holders")?;

    Ok(holders)
}

/// Get a specific token holder
pub async fn get_token_holder(
    pool: &PgPool,
    mint_address: &str,
    wallet: &str,
) -> Result<Option<TokenHolder>> {
    let holder = sqlx::query_as::<_, TokenHolder>(
        r#"
        SELECT 
            token_mint,
            user_wallet,
            balance,
            last_updated_slot,
            updated_at
        FROM token_holders 
        WHERE token_mint = $1 AND user_wallet = $2
        "#,
    )
    .bind(mint_address)
    .bind(wallet)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch token holder")?;

    Ok(holder)
}

// ==========================================
// TRANSACTION OPERATIONS (Audit Log)
// ==========================================

/// Insert transaction record
pub async fn insert_transaction(pool: &PgPool, transaction: &Transaction) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO transactions (
            signature, slot, block_time, signer, success, instruction_count
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (block_time, signature) DO NOTHING
        "#,
    )
    .bind(&transaction.signature)
    .bind(transaction.slot)
    .bind(transaction.block_time)
    .bind(&transaction.signer)
    .bind(transaction.success)
    .bind(transaction.instruction_count)
    .execute(pool)
    .await
    .context("Failed to insert transaction")?;

    Ok(())
}

// ==========================================
// ANALYTICS / STATS QUERIES
// ==========================================

/// Get total trades count for a token
pub async fn get_token_trade_count(pool: &PgPool, mint_address: &str) -> Result<i64> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) as count FROM trades WHERE token_mint = $1
        "#,
    )
    .bind(mint_address)
    .fetch_one(pool)
    .await
    .context("Failed to get trade count")?;

    let count: i64 = row.try_get("count")?;
    Ok(count)
}

/// Get 24h volume for a token
pub async fn get_24h_volume(pool: &PgPool, mint_address: &str) -> Result<Decimal> {
    let row = sqlx::query(
        r#"
        SELECT COALESCE(SUM(sol_amount), 0) as volume
        FROM trades 
        WHERE token_mint = $1 
        AND timestamp > NOW() - INTERVAL '24 hours'
        "#,
    )
    .bind(mint_address)
    .fetch_one(pool)
    .await
    .context("Failed to get 24h volume")?;

    let volume: Decimal = row.try_get("volume")?;
    Ok(volume)
}
