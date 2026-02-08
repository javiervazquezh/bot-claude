# Exit Strategy & Trailing Stop Optimization Analysis

**Date:** 2026-02-08
**Analyst:** Exit Strategy Agent (Task #2)
**Scope:** All exit logic across backtest, executor, and risk manager

---

## 1. Executive Summary

The exit logic contains one **critical architectural flaw** (backtest/live trailing stop mismatch), several **structural weaknesses** in stop placement, and multiple **missed opportunities** for letting winners run. The current system exits only via fixed StopLoss and TakeProfit in backtests, with trailing stops and time limits only active in live trading. This means **backtest results do not reflect live behavior**, making all backtest metrics unreliable for live performance prediction.

**Key findings:**
- CRITICAL: No trailing stop, time limit, or signal-based exit in backtest engine
- Backtest R:R is 2.45:1 (good), but live will be worse due to trailing stop cutting winners early
- 21.2% of stop-loss trades lose more than 8% (fat tail risk)
- Fixed TP cuts 40% of trades that would have gained 15%+ (some up to 60.9%)
- ATR-based stops are correctly placed but not dynamically updated after entry
- `ATRTrailingStop` struct exists in `atr.rs` but is never used anywhere

---

## 2. Current Exit Logic Architecture

### 2.1 Backtest Engine (`src/engine/backtest.rs`)

**`check_stops()` (lines 571-596):**
- Checks `candle.low <= stop_loss` for SL (correct for longs)
- Checks `candle.high >= take_profit` for TP (correct for longs)
- Executes at the SL/TP price itself, not at the candle extremes (realistic)
- **NO trailing stop logic**
- **NO time-based exit**
- **NO signal-based exit** (sell signals from strategies are processed but only close positions via `process_signal`, not through stop logic)

**`check_stops()` ordering (line 329):**
```
self.check_stops(pair, &candle)?;  // Line 329 - stops checked BEFORE strategy
self.run_strategy(pair, price, candle.open_time)?;  // Line 333 - strategy runs after
```
This is correct -- stops are checked before generating new signals, preventing a position from being both stopped and re-signaled on the same candle.

**Position price update (line 328):**
```
self.portfolio.update_position_price(pair, price);  // Uses candle.close
self.check_stops(pair, &candle)?;  // Then checks candle.low/high
```
`update_position_price` uses `candle.close` which updates `peak_pnl_pct` based on close price. But `check_stops` correctly uses `candle.low` for SL and `candle.high` for TP. However, `peak_pnl_pct` is tracked against close price, not intra-candle high -- this means the peak PnL may be understated since intra-candle highs are not captured by `update_price()`.

### 2.2 Live Executor (`src/engine/executor.rs`)

**`check_position_exits()` (lines 226-333):**
Three-layer exit check:
1. **Fixed SL** (`position.should_stop_loss()`) -- uses `current_price <= sl`
2. **Fixed TP** (`position.should_take_profit()`) -- uses `current_price >= tp`
3. **RiskManager trailing stop + time limit** (`should_close_position()`)

**Critical difference from backtest:**
- Live uses `current_price` (single tick), backtest uses `candle.low`/`candle.high`
- Live has trailing stop (peak > 5%, trail 2%), backtest does not
- Live has max holding time check, backtest does not
- Live checks RiskManager's `default_stop_loss_pct` and `default_take_profit_pct` as percentage-based overrides, backtest does not

### 2.3 Risk Manager (`src/risk/manager.rs`)

**`should_close_position()` (lines 200-233):**
```rust
// Percentage-based stop loss (separate from strategy SL)
if pnl_pct < -limits.default_stop_loss_pct { return StopLoss }

// Percentage-based take profit (separate from strategy TP)
if pnl_pct > limits.default_take_profit_pct { return TakeProfit }

// Time limit
if holding_duration_hours > limits.max_holding_hours { return TimeLimit }

// Trailing stop: hardcoded 5% activation, 2% trail
if peak_pnl_pct > 5 {
    if pnl_pct < peak_pnl_pct - 2 { return TrailingStop }
}
```

**Issues with this design:**
- Hardcoded thresholds (5% activation, 2% trail) not configurable
- Not ATR-adaptive -- same thresholds for BTC (low vol) and SOL (high vol)
- Completely absent from backtest, so untested

---

## 3. Quantitative Analysis from Backtest Results (358 trades, 2019-2025)

### 3.1 Exit Reason Distribution

| Exit Reason | Count | % | Win Rate | Avg PnL/Trade | Total PnL |
|---|---|---|---|---|---|
| StopLoss | 193 | 53.9% | 0.0% | -$1,020 | -$196,861 |
| TakeProfit | 165 | 46.1% | 100.0% | +$1,648 | +$271,999 |
| TrailingStop | 0 | 0.0% | N/A | N/A | N/A |
| TimeLimit | 0 | 0.0% | N/A | N/A | N/A |
| Signal | 0 | 0.0% | N/A | N/A | N/A |

**No trailing stop, time limit, or signal exits in backtest.** This is the single biggest discrepancy.

### 3.2 Stop-Loss PnL Distribution

| Range | Count | % of SL trades |
|---|---|---|
| > -3% | 18 | 9.3% |
| -3% to -8% | 134 | 69.4% |
| <= -8% | 41 | 21.2% |

**Problem:** 21.2% of SL trades lose more than 8%, with worst case -31.62%. These fat-tail losses (41 trades) collectively do outsized damage. The median SL loss is -5.27%, but the tail extends to -31.62%.

**Root cause:** Stop-loss is set at entry and never tightened. In a slow downtrend, price can gap through the SL level or the SL can be set too wide (especially for strategies with 2.0x ATR stops like RSIDivergence).

### 3.3 Take-Profit PnL Distribution

| Range | Count | % of TP trades |
|---|---|---|
| < 5% | 8 | 4.8% |
| 5% to 15% | 91 | 55.2% |
| >= 15% | 66 | 40.0% |

**Key insight:** 40% of winning trades gain 15%+ (some up to 60.94%). These big winners drive most of the profit. The fixed TP caps upside -- some of these 60.94% gains might have gone higher if not for the TP ceiling.

However, the counterpoint is that without TP, some of those wins might have reversed. The question is whether a trailing stop would capture more upside than a fixed TP.

### 3.4 Holding Time Analysis

| Exit Reason | Avg Hours | Min Hours | Max Hours |
|---|---|---|---|
| StopLoss | 136.5h | 4.0h | 1,292h |
| TakeProfit | 190.8h | 8.0h | 844h |

**Problem:** Some SL trades are held for 1,292 hours (54 days!) before stopping out. This represents massive capital lock-up. A time-based exit or tightening stop would free capital sooner.

### 3.5 Per-Pair Exit Analysis

| Pair | Trades | TP | SL | Win Rate |
|---|---|---|---|---|
| BTCUSDT | 127 | 55 | 72 | 43.3% |
| ETHUSDT | 122 | 58 | 64 | 47.5% |
| SOLUSDT | 109 | 52 | 57 | 47.7% |

BTC has the lowest win rate (43.3%) but strategies use the same ATR multipliers. BTC's lower volatility means tighter absolute stops, making it more susceptible to noise-induced stops.

---

## 4. Specific Problems Found

### Problem 1: CRITICAL -- No Trailing Stop in Backtest Engine
**File:** `src/engine/backtest.rs`
**Impact:** HIGH -- Backtest results are not predictive of live performance

The backtest engine's `check_stops()` (lines 571-596) only checks fixed SL/TP. The trailing stop logic in `RiskManager::should_close_position()` (lines 200-233 of `src/risk/manager.rs`) is never called during backtesting.

This means:
- In backtest: a trade that peaks at +20% PnL will either hit TP or eventually reverse to hit SL
- In live: the same trade would be closed by trailing stop once it drops 2% from peak
- **The entire backtest metric suite is optimistic** because some live trades that would be trailing-stopped as small wins appear as full TP wins in backtest

### Problem 2: No Time-Based Exit in Backtest
**File:** `src/engine/backtest.rs`
**Impact:** MEDIUM -- Holding time unlimited in backtest, limited in live

The `max_holding_hours` from `RiskLimits` is only checked in `executor.rs`. In backtest, positions can be held for 1,292 hours. In live with moderate settings (`max_holding_hours: 72`), these long trades would be force-closed, likely at a loss.

### Problem 3: Fixed Trailing Stop Thresholds Not ATR-Adaptive
**File:** `src/risk/manager.rs:225-230`
**Impact:** MEDIUM -- Same thresholds for all volatility levels

Hardcoded `peak_pnl_pct > 5` and trail of `2%` is:
- Too tight for SOL (high volatility, normal 5-candle swings can exceed 2%)
- Possibly too loose for BTC (low volatility, a 2% drop from peak is already significant)

### Problem 4: ATRTrailingStop Struct is Dead Code
**File:** `src/indicators/atr.rs:143-196`
**Impact:** MEDIUM -- Well-implemented ATR trailing stop exists but is never used

The `ATRTrailingStop` struct (lines 143-196) implements a proper ATR-based trailing stop that:
- Ratchets the stop upward for longs (never moves down)
- Uses configurable ATR multiplier
- Tracks stop price state

This is exactly what the backtest and live engine need, but it is never instantiated anywhere in the codebase.

### Problem 5: `peak_pnl_pct` Tracks Close Price, Not Intra-Candle High
**File:** `src/types/position.rs:71-78` and `src/engine/backtest.rs:328`
**Impact:** LOW-MEDIUM -- Peak PnL is slightly understated

`position.update_price(price)` is called with `candle.close`, but the actual intra-candle high might have been higher. When trailing stop logic is added to backtest, the peak PnL should incorporate `candle.high` for longs.

### Problem 6: Stop-Loss Never Tightened After Entry
**File:** `src/engine/backtest.rs:571-596`
**Impact:** MEDIUM -- 21.2% of SL trades lose > 8%

Once a position is opened with a fixed SL, it never changes. As price moves favorably, the SL should be moved to breakeven or better. The 41 trades losing > 8% represent preventable losses.

### Problem 7: `partial_close_position()` is Stubbed Out
**File:** `src/engine/portfolio.rs:272-274`
**Impact:** LOW-MEDIUM -- No partial exits available

```rust
pub fn partial_close_position(&mut self, ...) -> Result<Decimal> {
    Err(anyhow::anyhow!("Partial close not supported - position management removed"))
}
```

Partial exits (e.g., close 50% at 1.5R, trail the rest) are a proven technique for improving risk-adjusted returns, but the infrastructure has been removed.

---

## 5. Concrete Recommendations

### Recommendation 1: Add Trailing Stop + Time Limit to Backtest Engine (CRITICAL)
**File:** `src/engine/backtest.rs`, modify `check_stops()` around line 571
**Expected Impact:** Aligns backtest with live behavior. Likely reduces total return slightly but makes metrics trustworthy.

Add after the existing TP check in `check_stops()`:

```rust
fn check_stops(&mut self, pair: TradingPair, candle: &Candle) -> Result<()> {
    let position = match self.portfolio.get_position_for_pair(pair) {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    // Existing SL check (candle.low)
    if let Some(sl) = position.stop_loss {
        if candle.low <= sl {
            return self.close_position_internal(&position, sl, candle.open_time, ExitReason::StopLoss);
        }
    }

    // Existing TP check (candle.high)
    if let Some(tp) = position.take_profit {
        if candle.high >= tp {
            return self.close_position_internal(&position, tp, candle.open_time, ExitReason::TakeProfit);
        }
    }

    // NEW: Trailing stop (mirror live behavior)
    let pnl_pct = position.pnl_percentage();
    let peak_pnl_pct = position.peak_pnl_pct;
    if peak_pnl_pct > dec!(5) {
        let trailing_stop_level = peak_pnl_pct - dec!(2);
        if pnl_pct < trailing_stop_level {
            return self.close_position_internal(&position, candle.close, candle.open_time, ExitReason::TrailingStop);
        }
    }

    // NEW: Max holding time
    let holding_hours = (candle.open_time - position.opened_at).num_hours();
    if holding_hours > 72 {  // Match RiskLimits::moderate().max_holding_hours
        return self.close_position_internal(&position, candle.close, candle.open_time, ExitReason::TimeLimit);
    }

    Ok(())
}
```

Also add `TrailingStop` to `ExitReason` enum in `src/engine/results.rs` if not already present.

### Recommendation 2: Use ATRTrailingStop Instead of Fixed Percentage Trail
**File:** `src/engine/backtest.rs` (new field), `src/risk/manager.rs` (modify `should_close_position`)
**Expected Impact:** Better trailing stop that adapts to each asset's volatility. SOL gets wider trail, BTC gets tighter trail.

Replace the hardcoded `peak > 5%, trail 2%` with per-position ATR trailing stops:

```rust
// In BacktestEngine, add per-position ATR trailing stops
trailing_stops: HashMap<String, ATRTrailingStop>,

// When opening position:
let trailing = ATRTrailingStop::new(14, dec!(2.5), true); // 2.5x ATR trail
self.trailing_stops.insert(position.id.clone(), trailing);

// In check_stops, after existing checks:
if let Some(trail) = self.trailing_stops.get_mut(&position.id) {
    trail.update(candle.high, candle.low, candle.close);
    if trail.is_stopped(candle.low) {
        let stop_price = trail.stop_price().unwrap_or(candle.close);
        return self.close_position_internal(&position, stop_price, candle.open_time, ExitReason::TrailingStop);
    }
}
```

**Suggested multipliers by asset:**
- BTC: 2.0x ATR (tighter, lower volatility)
- ETH: 2.5x ATR (moderate)
- SOL: 3.0x ATR (wider, higher volatility)

### Recommendation 3: Breakeven Stop After 1R Profit
**File:** `src/engine/backtest.rs`, inside `check_stops()`
**Expected Impact:** Eliminates the 21.2% of SL trades that lose > 8%. Converts some losses to breakeven exits.

After a position is 1R in profit (i.e., pnl >= stop distance), move SL to breakeven:

```rust
// In check_stops, before checking SL:
if let Some(sl) = position.stop_loss {
    let risk_distance = (position.entry_price - sl).abs();
    let current_profit = candle.close - position.entry_price; // for longs

    // Move SL to breakeven once 1R achieved
    if current_profit >= risk_distance {
        let new_sl = position.entry_price + (position.entry_fee / position.quantity); // breakeven + fees
        if new_sl > sl {
            self.portfolio.get_position_for_pair_mut(pair)
                .map(|p| p.stop_loss = Some(new_sl));
        }
    }
}
```

### Recommendation 4: Replace Fixed TP with Trailing-Only Exit for Strong Signals
**File:** `src/engine/backtest.rs`, `check_stops()`
**Expected Impact:** Lets the 40% of TP trades that gain 15%+ potentially run to 25-50%+

For high-confidence signals (confidence > 0.75), consider removing the fixed TP and relying solely on the ATR trailing stop. This lets trend-following winners run while the trailing stop protects profits.

```rust
// When opening position with high confidence:
if signal.confidence > dec!(0.75) {
    // Don't set take_profit, let trailing stop manage exit
    position.take_profit = None;
}
```

### Recommendation 5: Dynamic Trailing Stop Activation Based on ATR
**File:** `src/risk/manager.rs:224-230`
**Expected Impact:** Better adaptation to market conditions

Replace hardcoded thresholds with ATR-relative ones:

```rust
pub async fn should_close_position(
    &self,
    pnl_pct: Decimal,
    peak_pnl_pct: Decimal,
    holding_duration_hours: i64,
    atr_pct: Option<Decimal>,  // NEW: ATR as % of price
) -> Option<CloseReason> {
    // ...existing checks...

    // Trailing stop: activation = 3x ATR%, trail = 1.5x ATR%
    let (activation, trail) = match atr_pct {
        Some(atr) => (atr * dec!(3), atr * Decimal::new(15, 1)),
        None => (dec!(5), dec!(2)),  // fallback to current behavior
    };

    if peak_pnl_pct > activation {
        let trailing_stop_level = peak_pnl_pct - trail;
        if pnl_pct < trailing_stop_level {
            return Some(CloseReason::TrailingStop);
        }
    }

    None
}
```

### Recommendation 6: Update `peak_pnl_pct` with Intra-Candle High
**File:** `src/engine/backtest.rs`, around line 328
**Expected Impact:** More accurate peak tracking for trailing stop activation

```rust
// In process_candle, before check_stops:
// Update peak PnL using candle high (not just close)
if let Some(pos) = self.portfolio.get_position_for_pair_mut(pair) {
    let high_pnl_pct = ((candle.high - pos.entry_price) / pos.entry_price) * dec!(100);
    if high_pnl_pct > pos.peak_pnl_pct {
        pos.peak_pnl_pct = high_pnl_pct;
    }
}
self.portfolio.update_position_price(pair, price);
self.check_stops(pair, &candle)?;
```

### Recommendation 7: Implement Partial Exits (Lower Priority)
**File:** `src/engine/portfolio.rs:272-274`, `src/engine/backtest.rs`
**Expected Impact:** Lock in partial profits at 1.5R, trail remainder. Reduces variance.

Re-implement `partial_close_position()`:

```rust
pub fn partial_close_position(&mut self, position_id: &str, close_fraction: Decimal, price: Decimal) -> Result<Decimal> {
    let position = self.positions.get_mut(position_id)
        .ok_or_else(|| anyhow::anyhow!("Position not found"))?;

    let close_qty = position.quantity * close_fraction;
    let pnl = (price - position.entry_price) * close_qty;

    position.quantity -= close_qty;
    position.realized_pnl += pnl;

    // Return USDT from partial close
    self.update_balance("USDT", close_qty * price);

    Ok(pnl)
}
```

Then in backtest `check_stops()`:
- At 1.5R profit: close 50% of position, move SL to breakeven
- Let remaining 50% ride with ATR trailing stop

---

## 6. Priority-Ordered Implementation Plan

| Priority | Recommendation | Expected Impact | Effort |
|---|---|---|---|
| 1 | Add trailing stop + time limit to backtest (Rec 1) | Critical alignment fix | Low |
| 2 | Breakeven stop after 1R (Rec 3) | Reduces fat-tail losses | Low |
| 3 | Use ATRTrailingStop (Rec 2) | Better volatility adaptation | Medium |
| 4 | Update peak PnL with candle high (Rec 6) | More accurate trailing | Low |
| 5 | Remove fixed TP for strong signals (Rec 4) | Lets winners run | Low |
| 6 | Dynamic trailing activation (Rec 5) | Better live adaptation | Medium |
| 7 | Partial exits (Rec 7) | Variance reduction | High |

---

## 7. Expected Overall Impact

**Current backtest metrics (358 trades, 2019-2025):**
- Win Rate: 46.1% | R:R: 2.45:1 | Profit Factor: 1.38

**After Rec 1 (trailing stop in backtest):** Total return will likely decrease 10-20% because some current TP wins will become trailing stop exits at lower levels. But this is the **true** performance.

**After Rec 1 + Rec 3 (breakeven stop):** The 41 trades losing > 8% (totaling roughly -$50K in losses) would be partially mitigated. Expected to recover $15-25K by converting deep losses to breakeven exits.

**After Rec 1 + Rec 3 + Rec 4 (no fixed TP for strong signals):** The 66 trades that gained 15%+ could run further. If just 20% of these gain an extra 10% on average, that's approximately +$20K.

**Net expected improvement:** Even after the honest trailing stop reduces apparent returns, the structural fixes (breakeven stop + letting winners run) should recover and exceed the reduction, resulting in a more robust and genuinely profitable strategy.

---

## 8. Code Location Quick Reference

| Component | File | Lines | Purpose |
|---|---|---|---|
| `check_stops()` | `src/engine/backtest.rs` | 571-596 | Backtest SL/TP checking |
| `open_position()` | `src/engine/backtest.rs` | 457-555 | Position entry + ATR sizing |
| `close_position_internal()` | `src/engine/backtest.rs` | 598-650 | Position exit + PnL calc |
| `process_candle()` | `src/engine/backtest.rs` | 300-368 | Main candle processing loop |
| `check_position_exits()` | `src/engine/executor.rs` | 226-333 | Live exit checking (3-layer) |
| `should_close_position()` | `src/risk/manager.rs` | 200-233 | Trailing stop + time limit |
| `ATRTrailingStop` | `src/indicators/atr.rs` | 143-196 | Unused ATR trailing stop |
| `ExitReason` enum | `src/engine/results.rs` | 187-194 | Exit reason tracking |
| `Position::update_price()` | `src/types/position.rs` | 71-78 | Price + peak PnL update |
| `partial_close_position()` | `src/engine/portfolio.rs` | 272-274 | Stubbed partial exit |
| `RiskLimits` | `src/risk/limits.rs` | 4-28 | Configurable risk thresholds |
