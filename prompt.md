Build a production-ready automated cryptocurrency trading bot in Rust that trades BTC, ETH, and SOL on Binance.US, starting with paper trading mode.

## Core Requirements

**Target Platform:** Binance.US API
**Mode:** Paper Trading (simulated) → Live Trading (future)
**Trading Pairs:** BTCUSDT, ETHUSDT, SOLUSDT
**Recommended Capital:** $1,500 - $2,500
**Language:** Rust
**Goal:** Maximize profit through volatility capture with disciplined risk management

## Asset Characteristics & Strategy Mapping

### BTC (BTCUSDT)
- **Role:** Market leader, portfolio anchor
- **Behavior:** Sets direction for entire crypto market
- **Best strategies:** Trend following, breakout, support/resistance
- **Position sizing:** Up to 40% of capital
- **Typical hold time:** Hours to days

### ETH (ETHUSDT)
- **Role:** Secondary leader, BTC beta play
- **Behavior:** Follows BTC but with higher volatility, DeFi narratives
- **Best strategies:** BTC correlation, momentum, ETH/BTC ratio trading
- **Position sizing:** Up to 35% of capital
- **Typical hold time:** Hours to days

### SOL (SOLUSDT)
- **Role:** High-beta momentum asset
- **Behavior:** Amplifies BTC moves 1.5-2.5x, prone to sharp reversals
- **Best strategies:** Momentum, mean reversion on extremes, volume breakouts
- **Position sizing:** Up to 25% of capital (higher risk)
- **Typical hold time:** Minutes to hours

## Architecture Components

### 1. Core Types
```rust
