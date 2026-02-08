# Signal Quality & Entry Logic Analysis

## Executive Summary

After deep analysis of the signal generation pipeline, I identified **11 concrete issues** across the CombinedStrategy ensemble, individual sub-strategies, and the backtest engine's signal filtering. The most impactful problems are: (1) Neutral signals diluting ensemble strength, (2) a broken aggregation math that almost never produces buy signals, (3) the BTC correlation strategy updating ATR with the wrong candle data, and (4) strategies conflicting with each other inside the same ensemble. Combined, these issues explain the 42.5% win rate and why the conservative config produces 0 trades.

---

## Problem 1: Neutral Signals Dilute Ensemble Strength (CRITICAL)

**File:** `src/strategies/combined.rs:211-292`

When a sub-strategy returns `Signal::Neutral` (confidence ~30%), it still gets included in the `aggregate_signals()` calculation because the CombinedStrategy calls `strategy.analyze()` and pushes **any** returned signal (line 313). Since neutral signals have `strength() == 0`, they drag the weighted average toward zero, making it nearly impossible for the ensemble to reach the `avg_strength > 1.5` threshold needed for `StrongBuy` (line 265) or even the `> 0.5` threshold for `Buy` (line 267).

**Impact:** Every sub-strategy that isn't signaling a buy/sell actively works against the strategies that are. For BTC's 3-strategy ensemble, if only TrendStrategy fires a StrongBuy (strength=2) while Breakout and MeanReversion return Neutral (strength=0), the weighted average is: `(2 * 0.45 * conf + 0 * 0.35 * 0.30 + 0 * 0.20 * 0.40) / 1.0`. Even at conf=0.85, avg_strength = 0.77 -- just barely a `Buy`, never a `StrongBuy`.

**Recommendation:** Filter out Neutral signals before aggregation. Only aggregate non-Neutral signals. If zero non-Neutral signals exist, return Neutral. This is the single highest-impact change.

```
// In aggregate_signals(), skip Neutral before the loop:
let active_signals: Vec<_> = signals.iter()
    .enumerate()
    .filter(|(_, s)| !matches!(s.signal, Signal::Neutral))
    .collect();
```

**Expected impact:** +5-10% win rate. Signals that do fire will carry their full weight instead of being diluted.

---

## Problem 2: Aggregation Math Almost Never Produces Buy Signals (CRITICAL)

**File:** `src/strategies/combined.rs:229-275`

The weighted strength calculation on line 230 multiplies `strength * weight * confidence`:
```rust
weighted_strength += strength * weight * signal.confidence;
```

For a `StrongBuy` (strength=2) with weight=0.45 and confidence=0.85:
- contribution = 2 * 0.45 * 0.85 = 0.765

Then avg_strength = weighted_strength / total_weight. Even with perfect alignment of all strategies at StrongBuy:
- BTC: (2*0.45*0.85 + 2*0.35*0.80 + 2*0.20*0.80) / 1.0 = (0.765 + 0.56 + 0.32) / 1.0 = 1.645

That's barely above the `> 1.5` threshold for StrongBuy! With realistic mixed signals, the average will rarely exceed 0.5. The `confidence` term double-penalizes weak signals (low confidence AND strength both reduce the weighted sum).

**Recommendation:** Either:
- (A) Remove the confidence multiplication from weighted_strength and use it only for the avg_confidence output, OR
- (B) Lower the thresholds to match the actual output range (e.g., StrongBuy at > 0.8, Buy at > 0.3)

Option (A) is cleaner:
```rust
weighted_strength += Decimal::from(signal.signal.strength() as i32) * weight;
// Keep confidence tracked separately for the output
```

**Expected impact:** Combined with Problem 1 fix, this should double the number of valid buy entries.

---

## Problem 3: BTC Correlation Strategy Updates ATR From Wrong Data (MODERATE)

**File:** `src/strategies/combined.rs:424-426`

In `BTCCorrelationStrategy::analyze()`, the ATR is updated from the ETH candle buffer passed as the `candles` parameter:
```rust
self.atr.update(current.high, current.low, current.close); // ETH candle
```

But this ATR is then used to set stop-loss and take-profit levels for the correlation signal (line 456-467). The strategy is supposed to capture BTC momentum translating to ETH, but the SL/TP levels use ETH's ATR, which is fine for execution but the ATR update only happens when `analyze()` is called. Since BTCCorrelationStrategy isn't in the main strategies HashMap, it only gets called indirectly through CombinedStrategy, which means it only sees ETH candles, not BTC candles.

More importantly, the `calculate_btc_momentum()` method on line 384 averages BTC candle changes over `lag_periods + 1` candles, which for lag_periods=2 means only 3 candles. This is extremely noisy on 5-minute data.

**Recommendation:** Increase `lag_periods` to 4-6 for 5-minute timeframe, and increase the "strong" threshold from 1% to 1.5%.

**Expected impact:** Fewer false BTC correlation signals for ETH. The 15% weight on a noisy signal currently injects noise into ETH's otherwise best-performing ensemble.

---

## Problem 4: SOL Ensemble Has No Trend Strategy (MODERATE)

**File:** `src/strategies/combined.rs:92-119`

SOL's ensemble is: Momentum (35%), VolumeBreakout (25%), MeanReversion (25%), RSIDivergence (15%). There is no TrendStrategy.

SOL is described as a "high-beta momentum asset," but in the Aug 2025 - Feb 2026 backtest, SOL lost $79. The problem is that MomentumStrategy and VolumeBreakoutStrategy often contradict each other -- Momentum wants to ride trends while VolumeBreakout fires on any volume spike regardless of trend direction. MeanReversion then wants to fade both.

Without a TrendStrategy anchor, SOL's ensemble has no directional bias filter. When the market trends, MeanReversion fires false reversal signals, and VolumeBreakout fires on counter-trend volume spikes.

**Recommendation:** Replace RSIDivergenceStrategy (15%) with TrendStrategy (20%), and reduce MeanReversion to 15%:
```rust
// SOL ensemble
TrendStrategy (20%)
MomentumStrategy (35%)
VolumeBreakoutStrategy (25%)
MeanReversionStrategy (20%)  // reduced from 25%
```

Also set `layout.trend_idx: Some(0)` for SOL so regime detection can properly boost/reduce trend weight.

**Expected impact:** Should reduce SOL false signals by filtering counter-trend entries. Expected to improve SOL from net negative to breakeven or slightly positive.

---

## Problem 5: MeanReversion Take Profit Is Too Conservative (MODERATE)

**File:** `src/strategies/mean_reversion.rs:174-186`

MeanReversion's take profit is set to the Bollinger middle band:
```rust
let tp = bb_middle;
```

For a long entry at the lower band, the risk (entry to SL) is: `bb_lower - (bb_lower - atr * 0.5)` = `atr * 0.5`. The reward (entry to TP) is: `bb_middle - bb_lower`. On standard 2-std-dev Bollinger bands, `bb_middle - bb_lower` equals roughly 2 standard deviations of price. But in ranging markets (where mean reversion should work best), this distance is often smaller than expected.

The critical issue: the R:R ratio is often below 2.0, which means the backtest engine's `min_risk_reward: dec!(2.0)` filter rejects these signals at `src/engine/backtest.rs:411-414`. For conservative config with `min_risk_reward: dec!(2.5)`, MeanReversion signals are almost never accepted.

**Recommendation:** Change MeanReversion TP to extend past the middle band:
```rust
// Instead of just middle band, target middle + 50% toward upper band
let tp = bb_middle + (bb_upper - bb_middle) * Decimal::new(5, 1); // midpoint to upper
```

This gives a better R:R and more mean-reversion trades pass the filter.

**Expected impact:** +20-30% more mean reversion trades qualifying, especially in low-volatility periods where mean reversion should shine.

---

## Problem 6: Breakout Strategy SL Is Too Tight (MODERATE)

**File:** `src/strategies/trend.rs:313-325`

BreakoutStrategy's stop loss for a bullish breakout is:
```rust
let sl = highest - threshold; // threshold = ATR * 1.5
```

This places the SL at the old resistance level minus the breakout threshold. But breakout retests are normal -- price often pulls back to the breakout level before continuing. This SL is inside the normal retest zone, causing many breakout trades to stop out prematurely.

**Recommendation:** Use a wider SL for breakout entries:
```rust
let sl = highest - atr; // Simple: SL at 1 ATR below the breakout level
```

This gives more room for the typical post-breakout retest while keeping the TP at 2x threshold.

**Expected impact:** Breakout win rate improvement for BTC (currently 33.3% WR but 1.92 PF, meaning winners are big but too many losers from tight stops).

---

## Problem 7: Conservative Config Produces 0 Trades (CRITICAL USABILITY)

**File:** `src/engine/backtest.rs` and `src/main.rs:726-735`

Conservative config: `min_confidence: 0.70, min_risk_reward: 2.5`. No trades in 6 months.

Root cause analysis: The confidence math in each strategy maxes out around 80-85% for the sub-strategies, but the ensemble aggregation computes a **weighted average** of confidences (line 262):
```rust
let avg_confidence = total_confidence / total_weight;
```

Even if one strategy hits 85% confidence, the average with other strategies at 50-60% pulls it down to 60-65% -- below the 70% threshold.

**Recommendation:** Two changes:
1. Use `max confidence` from any sub-strategy instead of weighted average when determining whether the ensemble signal passes the threshold. The ensemble already selects the best entry/SL/TP levels from the highest-confidence sub-strategy (line 237-243), so it's inconsistent not to use max confidence for the pass/fail decision.
2. Lower conservative `min_confidence` to 0.65 (same as moderate) and differentiate conservative purely via position sizing and max_allocation.

**Expected impact:** Conservative config would produce trades, making it actually usable. Without this fix, the conservative preset is entirely broken.

---

## Problem 8: Momentum Strategy Has Overlapping/Conflicting Signals (LOW-MODERATE)

**File:** `src/strategies/momentum.rs:70-89`

The `analyze_momentum()` method has overlapping conditions. `is_bullish` (RSI > 50 && price > fast EMA) can be true simultaneously when `is_strong_bullish` fails (because RSI < 55 or price < slow EMA). This is fine. However, the method never checks RSI direction or RSI rate of change -- it only checks static levels.

A momentum strategy should care about whether RSI is rising or falling, not just its absolute level. An RSI of 55 that was 65 two candles ago is bearish momentum disguised as a bullish reading.

**Recommendation:** Track previous RSI values and add a momentum direction filter:
- Only signal StrongBuy if RSI is currently rising (RSI > RSI_prev by threshold)
- Only signal Buy if RSI is not declining

**Expected impact:** Filters out 15-25% of false momentum signals where RSI was previously higher and is declining through the bullish zone.

---

## Problem 9: VolumeBreakout Has No Directional Confirmation (LOW-MODERATE)

**File:** `src/strategies/momentum.rs:330-346`

VolumeBreakoutStrategy determines direction solely from `current.is_bullish()` -- whether the current candle close > open. This is a single-candle determination with no multi-candle context.

A volume spike on a slightly bullish candle could be distribution (large sellers filling), not accumulation. Without checking the preceding trend or multiple candle context, many volume breakout signals are directionally wrong.

**Recommendation:** Add a 3-candle trend check before determining direction:
```rust
let recent_3 = candles.last_n(3);
let net_change = recent_3.last()?.close - recent_3.first()?.close;
let is_bullish = net_change > Decimal::ZERO && current.is_bullish();
```

**Expected impact:** Moderate reduction in false VolumeBreakout signals for SOL.

---

## Problem 10: Regime Detection Weight Shift Doesn't Normalize (LOW)

**File:** `src/strategies/combined.rs:148-208`

The regime detection shifts weights by a flat 15% (line 158). For BTC's ensemble (45%, 35%, 20%), in a High-vol regime:
- Trend: 45% + 15% = 60%
- Breakout: 35% (unchanged)
- MeanReversion: 20% - 15% = 5%
- Total: 100% -- OK

But for SOL's ensemble (35%, 25%, 25%, 15%) in Extreme regime:
- Momentum: 35% + 10% = 45%
- Others each reduced by 10%/3 = 3.33%
- Total: 35+25+25+15 = 100, then +10 - 10 = 100 -- OK but weights can go negative

The `Extreme` path uses a 10% boost (line 189) rather than 15%, which is inconsistent. More importantly, negative weights are clamped to zero (line 202-206) but the total is never re-normalized to 100%. If any weight hits zero, the total weight drops below 100%, which means `total_weight` in `aggregate_signals` will be less than expected, inflating the avg_strength and avg_confidence.

**Recommendation:** After clamping, re-normalize weights to sum to 1.0:
```rust
let sum: Decimal = adjusted.iter().sum();
if sum > Decimal::ZERO {
    for w in adjusted.iter_mut() {
        *w = *w / sum;
    }
}
```

**Expected impact:** Minor, but prevents edge cases in extreme volatility where inflated confidence passes threshold checks incorrectly.

---

## Problem 11: No Cooldown Between Trades on Same Pair (LOW)

**File:** `src/engine/backtest.rs:438-454`

After closing a position (either by signal, stop-loss, or take-profit), the engine can immediately open a new position on the very next candle. This means a stop-loss exit can be followed by an immediate re-entry if the strategy still fires a buy signal, leading to "churn" -- multiple small losses in choppy markets.

**Recommendation:** Add a cooldown period (e.g., 12 candles = 1 hour on 5-min timeframe) after a position is closed before allowing re-entry on the same pair. Track `last_close_time` per pair.

**Expected impact:** Reduces trade churn during whipsaw periods. Expected to eliminate 10-20% of losing trades.

---

## Priority-Ordered Recommendations

| Priority | Problem | Expected Win Rate Impact | Effort |
|----------|---------|--------------------------|--------|
| 1 | Filter Neutral signals from ensemble aggregation (#1) | +5-10% | Low |
| 2 | Fix aggregation math: remove double-confidence penalty (#2) | +5-8% | Low |
| 3 | Fix conservative config: use max confidence (#7) | Enables conservative preset | Low |
| 4 | MeanReversion TP too conservative (#5) | +20-30% more MR trades | Low |
| 5 | Add TrendStrategy to SOL ensemble (#4) | SOL breakeven vs -$79 | Medium |
| 6 | Widen Breakout SL to prevent retest stopouts (#6) | +5% BTC breakout WR | Low |
| 7 | BTC correlation: increase lag, raise threshold (#3) | Fewer false ETH signals | Low |
| 8 | Add trade cooldown (#11) | -10-20% losing trades | Medium |
| 9 | Momentum: add RSI direction filter (#8) | -15% false momentum | Medium |
| 10 | VolumeBreakout: multi-candle direction (#9) | Fewer false SOL signals | Medium |
| 11 | Regime weight normalization (#10) | Edge case fix | Low |

## Combined Expected Impact

Implementing priorities 1-4 (all low effort) should:
- Increase win rate from ~42% to ~52-55%
- Make the conservative config functional (currently 0 trades)
- Improve Sharpe ratio from 0.18 to estimated 0.4-0.6
- SOL should move from -$79 to roughly breakeven with priority 5

Implementing all 11 fixes should push win rate to ~55-60% with the moderate config and significantly improve all three assets' performance.
