# Fee Impact & Trade Frequency Optimization Analysis

**Date:** 2026-02-08
**Backtest Period:** 2025-08-01 to 2026-02-08 (6.3 months)
**Initial Capital:** $2,000

---

## 1. Executive Summary

Fees are the single largest drag on profitability. In the Moderate scenario, the bot paid **$83.72 in fees to earn just $18.08 in net profit** -- a fee-to-profit ratio of **4.63x**. For every $1 of profit, $4.63 was spent on fees. The Aggressive scenario is worse: $104.42 in fees on a *net loss* of -$221.44.

The Conservative scenario (min_confidence=0.70, min_risk_reward=2.5) produced **zero trades**, which means the signal quality filters are too harsh at that level. The system's profitable range is extremely narrow: somewhere between 0.65 and 0.70 confidence, and the current 0.65 threshold allows too many marginal trades.

---

## 2. Fee/Slippage Model Analysis

### How fees are calculated (`src/engine/backtest.rs`)

**Entry (line 466):**
```
execution_price = price * (1 + slippage_rate)    // 0.05% adverse slippage
fee = quantity * execution_price * fee_rate       // 0.1% of notional
```

**Exit (line 606):**
```
execution_price = price * (1 - slippage_rate)     // 0.05% adverse slippage
fee = quantity * execution_price * fee_rate        // 0.1% of notional
```

**Total round-trip cost per trade:**
- Fee: 0.1% entry + 0.1% exit = **0.2% of notional**
- Slippage: 0.05% entry + 0.05% exit = **0.1% of notional**
- **Total: ~0.3% round-trip cost per trade**

### Are these rates realistic for Binance.US?

| Parameter | Backtest Value | Binance.US Actual | Assessment |
|-----------|---------------|-------------------|------------|
| Fee rate  | 0.1% (10 bps) | 0.1%-0.6% maker/taker | **Optimistic** -- new accounts pay 0.4-0.6%. Only VIP tiers get 0.1%. |
| Slippage  | 0.05% (5 bps) | Varies by pair/size | **Reasonable for BTC/ETH**, possibly **optimistic for SOL** on low liquidity |

**Key finding:** If actual fees are 0.4% (non-VIP), the real round-trip cost would be **0.9%** instead of 0.3%. This would turn the Moderate scenario from +$18.08 to approximately **-$150**.

---

## 3. Trade-by-Trade Fee Analysis (Moderate Scenario)

### 3.1 Fee Distribution

| Metric | Value |
|--------|-------|
| Total trades | 40 |
| Total fees | $83.72 |
| Average fee per trade | **$2.09** |
| Median fee per trade | ~$2.50 |
| Fee range | $0.40 - $2.91 |
| Average notional per trade | ~$1,045 |

### 3.2 Fee as Percentage of Gross P&L

| Component | Amount | As % of Gross Profit |
|-----------|--------|---------------------|
| Gross Profit (17 wins) | $709.89 | 100% |
| Gross Loss (23 losses) | $691.80 | 97.5% |
| Total Fees | $83.72 | **11.8% of gross profit eaten by fees** |
| Net Profit | $18.09 | 2.5% |

**Without fees, the gross edge would be $709.89 - $691.80 = $18.09. Fees consume $83.72 on top of that, bringing net to just $18.08. Fees are almost exactly equal to the gross edge.**

### 3.3 Breakeven Analysis

With current fee structure ($2.09 avg fee per trade):
- Average win: $41.76
- Average loss: $30.08
- Win rate needed to break even (ignoring fees): 41.9% (current: 42.5%)
- **Win rate needed to break even WITH fees:**

```
Breakeven WR = (AvgLoss + AvgFee) / (AvgWin + AvgLoss)
             = ($30.08 + $2.09) / ($41.76 + $30.08)
             = $32.17 / $71.84
             = 44.8%
```

**Current win rate (42.5%) is BELOW the breakeven win rate (44.8%).** The bot is marginally profitable only because some winning trades are large enough to overcome the fee drag.

---

## 4. Trade Holding Period Analysis

### 4.1 Holding Duration Distribution

From the 40 Moderate trades (entry_time to exit_time):

| Duration (hours) | Count | Win Rate | Avg PnL |
|-------------------|-------|----------|---------|
| 8h (2 candles)    | 8     | 25%      | -$19.40 |
| 12h (3 candles)   | 4     | 50%      | +$3.90  |
| 16h (4 candles)   | 7     | 57%      | +$14.20 |
| 20h (5 candles)   | 3     | 33%      | -$12.50 |
| 24-48h (6-12 candles) | 12 | 58%   | +$18.70 |
| 48-96h (12-24 candles) | 4 | 25%   | -$7.50  |
| 96h+ (24+ candles) | 2   | 0%       | -$26.70 |

### 4.2 Short-Duration Trade Problem (Churning)

**8-hour trades (2 candles = entry candle + next candle stop-out):**

These 8 trades are mostly rapid stop-outs:
1. Trade #6 (ETH Aug 15): -$45.67, 8h, StopLoss
2. Trade #11 (ETH Sep 11): -$27.88, 8h, StopLoss
3. Trade #10 (SOL Sep 10): -$29.45, 12h, StopLoss
4. Trade #21 (SOL Oct 21): -$43.09, 8h, StopLoss
5. Trade #30 (ETH Dec 5): -$43.20, 16h, StopLoss
6. Trade #39 (SOL Jan 12): -$27.27, 8h, StopLoss

**Pattern:** 8-hour trades have a 25% win rate and average -$19.40 loss. Each one costs ~$2.50 in fees for almost guaranteed loss. These are **entries into choppy/volatile conditions where the stop is hit on the very next candle.**

### 4.3 Rapid Re-entry Pattern (Trade Churning)

Several clusters show the bot exiting on stop-loss then immediately re-entering:

**Cluster 1 (Aug 12-15):** 4 trades in 3 days
- Aug 12: Open ETH (TP hit +$68.21)
- Aug 12: Open SOL (TP hit +$24.61)
- Aug 13: Open BTC (SL hit -$9.48)
- Aug 15: Open ETH (SL hit -$45.67) -- **immediate re-entry after BTC loss**

**Cluster 2 (Sep 10-14):** 4 trades in 4 days
- Sep 10: Open SOL (SL hit -$29.45) -- 12h hold
- Sep 11: Open ETH (SL hit -$27.88) -- 8h hold, **opened day after SOL loss**
- Sep 12: Open ETH (TP hit +$46.19) -- 20h hold
- Sep 12: Open ETH (SL hit -$30.93) -- 40h hold, **re-entered same day**

**Cluster 3 (Dec 3-11):** 4 trades in 8 days
- Dec 3: Open ETH (TP hit +$66.57) -- 20h
- Dec 5: Open ETH (SL hit -$43.20) -- 16h
- Dec 8: Open ETH (TP hit +$70.78) -- 24h
- Dec 9: Open ETH (SL hit -$43.39) -- 32h

**Fees for these 3 clusters alone: ~$30 (12 trades x ~$2.50)**

---

## 5. Consecutive Loss Analysis

### 5.1 Loss Streaks

| Streak Length | Occurrences | Total Fees During Streak | Total Loss During Streak |
|---------------|-------------|--------------------------|--------------------------|
| 1 loss | 5 | $12.50 | -$155 |
| 2 consecutive losses | 4 | $20.00 | -$248 |
| 3 consecutive losses | 1 | $7.50 | -$101 |

**Worst drawdown period:** Sep 10 - Oct 1 (5 of 6 trades are losses)
- Sep 10: SOL SL -$29.45
- Sep 11: ETH SL -$27.88
- Sep 12: ETH TP +$46.19 (brief respite)
- Sep 14: ETH SL -$30.93
- Sep 19: SOL SL -$33.41
- Sep 30: ETH SL -$30.50
- **Net for this period: -$105.98, Fees: ~$15**

### 5.2 No Cooldown After Losses

**Critical finding:** CombinedStrategy (`src/strategies/combined.rs`) has **NO cooldown mechanism**. Unlike ImprovedStrategy which has an 8-candle cooldown (`src/strategies/improved.rs:109-111`), the CombinedStrategy and BacktestEngine will immediately generate and accept new signals after a loss.

The ImprovedStrategy cooldown exists but is **NOT used in backtesting** because BacktestEngine uses CombinedStrategy via `create_strategies_for_pair()`.

---

## 6. Confidence Threshold Analysis

### 6.1 Current Settings Comparison

| Scenario | min_confidence | min_risk_reward | Trades | Net Profit | Fees |
|----------|---------------|-----------------|--------|------------|------|
| Conservative | 0.70 | 2.5 | **0** | $0 | $0 |
| Moderate | 0.65 | 2.0 | 40 | +$18.08 | $83.72 |
| Aggressive | 0.55 | 1.5 | 50 | -$221.44 | $104.42 |

### 6.2 The "Dead Zone" Problem

The jump from 0 trades (conservative at 0.70) to 40 trades (moderate at 0.65) reveals that **most signals cluster around 0.65-0.70 confidence**. This is a narrow band, and the strategies are not generating any truly high-confidence signals (>0.70).

**This means the confidence scores are poorly calibrated.** If the CombinedStrategy's maximum practical output is ~0.68, then a threshold of 0.70 filters everything while 0.65 lets everything through.

### 6.3 Estimated Impact of Raising min_confidence to 0.68

If we assume roughly linear distribution of signals between 0.65 and 0.70:
- Raising to 0.68 would filter ~60% of current trades: ~16 trades instead of 40
- Assuming the filtered trades are the lowest-quality ones (more likely to stop out):
  - Estimated win rate improvement: 42.5% -> ~50%
  - Estimated fee reduction: $83.72 -> ~$33.50
  - **Estimated net improvement: +$50-$80**

However, this requires the strategies to actually emit calibrated confidence values, which needs investigation.

---

## 7. Per-Pair Cost Analysis

### 7.1 Pair Profitability After Fees

| Pair | Trades | Net PnL | Fees (est.) | PnL Before Fees | Fee % of Gross |
|------|--------|---------|-------------|-----------------|----------------|
| BTCUSDT | 6 | +$31.77 | ~$8.93 | +$40.70 | 22% |
| ETHUSDT | 24 | +$65.03 | ~$57.13 | +$122.16 | 47% |
| SOLUSDT | 10 | -$78.71 | ~$17.66 | -$61.05 | N/A (net loss) |

### 7.2 Key Observations

- **ETH** generates the most trades (24/40 = 60%) and the most fees. Despite being profitable, nearly half its gross profit is consumed by fees.
- **SOL** is unprofitable even before fees (profit factor 0.55). Every SOL trade costs ~$1.77 in fees on a losing proposition.
- **BTC** has fewest trades (6) and best profit factor (1.92) -- suggesting BTC signals are higher quality but less frequent.

---

## 8. Timeframe Analysis: 4H vs Daily

### 8.1 Current 4H Timeframe

- Period: ~190 days = 1,140 four-hour candles per pair (3,420 total)
- 40 trades / 190 days = **~1.5 trades per week**
- Average holding: ~24 hours (6 candles)

### 8.2 Projected Daily (1D) Timeframe Impact

Moving to daily candles would:
- Reduce candle count by 6x (190 candles per pair instead of 1,140)
- Reduce signal frequency proportionally
- Expected trades: ~7-10 trades over 6 months
- Expected fees: ~$15-$20 (80% reduction)
- Slippage impact: identical (same entry/exit mechanics)

**Tradeoff:** Fewer but potentially higher-quality signals, since daily candles filter out intraday noise. However, stop distances would be wider (daily ATR > 4H ATR), which means larger per-trade risk.

**Estimated impact:** Fee savings of ~$60, but similar or slightly worse gross edge due to wider stops. Net result: likely **better** than current 4H due to fee savings.

---

## 9. Concrete Recommendations

### Recommendation 1: Add Cooldown to BacktestEngine (HIGH IMPACT)

**File:** `src/engine/backtest.rs`
**Location:** `process_signal()` method (line 392-455)

Add a per-pair cooldown tracker that prevents re-entry for N candles after a losing trade exit. The ImprovedStrategy has this at the strategy level (`src/strategies/improved.rs:109-111`) but it is not used in the CombinedStrategy path.

**Implementation:** Add `last_exit_candle: HashMap<TradingPair, u64>` and `last_exit_was_loss: HashMap<TradingPair, bool>` to BacktestEngine. In `process_signal()`, skip buy signals for a pair if `candles_processed - last_exit_candle[pair] < cooldown_candles` and the last exit was a loss.

**Suggested cooldown:** 6 candles (24 hours on 4H timeframe) after a stop-loss exit.

**Expected impact:** Eliminates ~8-10 rapid re-entry trades. Estimated fee savings: $20-$25. Estimated loss avoidance: $80-$120 (by avoiding the immediate re-entry stop-outs).

### Recommendation 2: Add Minimum Holding Period (MEDIUM IMPACT)

**File:** `src/engine/backtest.rs`
**Location:** `check_stops()` method (line 571-596)

Add a minimum holding period before stop-loss can trigger (e.g., 3 candles = 12 hours for 4H). This prevents the rapid 8-hour stop-outs that have 25% win rate.

**Expected impact:** The 8 trades with 8h duration would instead hold longer, giving trades time to work. Some would still stop out, but at more meaningful levels.

**Caution:** This increases risk per trade since the stop is delayed. Should be paired with tighter position sizing.

### Recommendation 3: Raise min_confidence to 0.68 (HIGH IMPACT)

**File:** `src/main.rs`
**Location:** Lines 761 (moderate config), 649 (save-to-db config), 859 (walk-forward config)

Change `min_confidence: dec!(0.65)` to `min_confidence: dec!(0.68)`.

**Expected impact:** Reduce trades from ~40 to ~16-20, filtering the lowest-quality signals. Estimated fee savings: $40-$50. Win rate improvement: +5-8%.

**Risk:** If confidence calibration is poor, this might filter good trades too. Need to verify by examining actual confidence values emitted by CombinedStrategy.

### Recommendation 4: Disable SOL Trading (HIGH IMPACT, SIMPLE)

**File:** `src/main.rs`
**Location:** Lines 753-756 (moderate pairs), 643-646 (save-to-db pairs)

Remove `TradingPair::SOLUSDT` from the pairs list.

SOL has a profit factor of 0.55 (loses $1.83 for every $1 gained) across all scenarios. It is consistently unprofitable.

**Expected impact:** Eliminates 10 losing trades. Fee savings: ~$18. Loss avoidance: ~$79. Total improvement: ~$97.

### Recommendation 5: Add Loss-After-Loss Filter (MEDIUM IMPACT)

**File:** `src/engine/backtest.rs`
**Location:** `process_signal()` method (line 392)

Track the last N trade outcomes. After 2 consecutive losses on any pair, increase the min_confidence threshold for that pair by 0.05 (temporary "cold streak" mode that resets after a win).

**Expected impact:** Reduces damage during choppy/trending-down markets when signals are unreliable. Estimated improvement: $30-$50 per 6-month period.

### Recommendation 6: Use Realistic Fee Rate (CRITICAL for live trading)

**File:** `src/engine/backtest.rs`
**Location:** Default config (line 44)

For realistic backtesting, change `fee_rate: dec!(0.001)` to `dec!(0.004)` unless using a VIP fee tier. Current backtests underestimate fees by 4x for non-VIP accounts.

**Impact on Moderate scenario with 0.4% fees:**
- Fee would be ~$335 instead of $83.72
- Net result: approximately -$317 (catastrophic loss)
- **The bot is NOT viable at standard Binance.US fee tiers**

### Recommendation 7: Consider Daily Timeframe (MEDIUM IMPACT, LONGER TERM)

**File:** `src/main.rs`
**Location:** Lines 751 (moderate timeframe)

Change `timeframe: TimeFrame::H4` to `timeframe: TimeFrame::D1` and adjust strategy parameters accordingly.

**Expected impact:** Reduces trade frequency by ~4-6x, proportional fee reduction. Requires re-tuning stop/take-profit distances for daily volatility.

---

## 10. Prioritized Implementation Order

| Priority | Recommendation | Estimated Net Impact | Effort |
|----------|---------------|---------------------|--------|
| 1 | Disable SOL trading | +$97/6mo | Trivial (delete 1 line) |
| 2 | Add cooldown after loss | +$100-$145/6mo | Small (add HashMap + check) |
| 3 | Raise min_confidence to 0.68 | +$40-$50/6mo | Trivial (change 1 constant) |
| 4 | Use realistic fee rate for testing | Prevents live losses | Trivial |
| 5 | Add loss-streak filter | +$30-$50/6mo | Medium |
| 6 | Daily timeframe experiment | Unknown (needs testing) | Medium |
| 7 | Minimum holding period | +$10-$20/6mo | Small |

**Combined estimated impact of top 3 changes:** Moderate scenario would go from +$18 to approximately **+$255-$292** net profit (14x improvement), with roughly half the trades and dramatically better risk-adjusted returns.

---

## 11. Slippage Model Assessment

The current slippage model (`0.05%` fixed) is **overly simplistic** but **not unreasonable** as an average:

- **BTC:** Slippage of 0.05% on $120K = $60 per BTC. Realistic for market orders on Binance.US with ~$500K daily volume at the top of book.
- **ETH:** Slippage of 0.05% on $4K = $2 per ETH. Reasonable.
- **SOL:** Slippage of 0.05% on $200 = $0.10 per SOL. Potentially optimistic during volatile periods when SOL spreads widen.

A more realistic model would use **variable slippage** based on position size relative to order book depth, but the current fixed model is acceptable for backtesting purposes.

---

## 12. Summary Table: Fee Impact by Scenario

| Metric | Conservative | Moderate | Aggressive |
|--------|-------------|----------|------------|
| Trades | 0 | 40 | 50 |
| Gross Profit | $0 | $709.89 | $789.99 |
| Gross Loss | $0 | $691.80 | $1,011.44 |
| Gross Edge | $0 | $18.09 | -$221.44 |
| Total Fees | $0 | $83.72 | $104.42 |
| **Net Profit** | **$0** | **$18.08** | **-$221.44** |
| Fee/Gross Profit | N/A | **11.8%** | **13.2%** |
| Fee/Net Profit | N/A | **463%** | N/A (loss) |

**The fundamental problem:** The bot's gross trading edge (~$18 over 6 months on $2000 capital) is razor-thin. Any significant fee burden destroys profitability. The strategies need to either (a) generate dramatically better signals with higher win rates, or (b) trade far less frequently so each trade carries more conviction and the fee count drops.
