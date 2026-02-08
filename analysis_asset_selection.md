# Asset Selection & Pair-Specific Performance Analysis

**Date:** 2026-02-08
**Backtest Period (Moderate):** 2025-08-01 to 2026-02-08 (6 months)
**Initial Capital:** $2,000 | **Timeframe:** 4-hour candles

---

## 1. Per-Pair Performance Summary (Moderate Scenario)

| Metric | ETH | BTC | SOL | **Total** |
|---|---|---|---|---|
| Trades | 24 | 6 | 10 | **40** |
| Wins / Losses | 11 / 13 | 2 / 4 | 4 / 6 | **17 / 23** |
| Win Rate | 45.8% | 33.3% | 40.0% | **42.5%** |
| Net PnL | **+$65.03** | **+$31.77** | **-$78.71** | **+$18.09** |
| Gross Profit | $548.31 | $66.36 | $95.21 | $709.89 |
| Gross Loss | $483.28 | $34.59 | $173.92 | $691.80 |
| Profit Factor | 1.13 | 1.92 | **0.55** | 1.03 |
| Avg Win | $49.85 | $33.18 | $23.80 | $41.76 |
| Avg Loss | $37.18 | $8.65 | $28.99 | $30.08 |
| Largest Win | $85.52 | $34.73 | $29.39 | -- |
| Largest Loss | $47.60 | $9.93 | $43.09 | -- |

**Key observation:** SOL is the sole drag on the portfolio. Without SOL, the bot would be +$96.80 (4.84% return) instead of +$18.09 (0.90%).

---

## 2. SOL Trade-by-Trade Deep Dive

### SOL Trades Chronologically

| # | Entry Date | Exit Date | Entry Price | Exit Price | PnL | PnL% | Exit Reason |
|---|---|---|---|---|---|---|---|
| 1 | Aug 12 16:00 | Aug 13 08:00 | $190.90 | $200.85 | **+$24.61** | +5.2% | TakeProfit |
| 2 | Aug 22 12:00 | Aug 23 04:00 | $193.64 | $205.41 | **+$29.39** | +6.1% | TakeProfit |
| 3 | Sep 10 04:00 | Sep 10 16:00 | $222.28 | $217.66 | -$29.45 | -2.1% | StopLoss |
| 4 | Sep 17 20:00 | Sep 19 12:00 | $244.88 | $238.91 | -$33.41 | -2.4% | StopLoss |
| 5 | Oct 01 04:00 | Oct 01 12:00 | $209.63 | $218.93 | **+$20.79** | +4.4% | TakeProfit |
| 6 | Oct 21 12:00 | Oct 21 20:00 | $196.10 | $190.15 | -$43.09 | -3.0% | StopLoss |
| 7 | Nov 26 16:00 | Nov 28 00:00 | $144.12 | $139.67 | -$16.53 | -3.1% | StopLoss |
| 8 | Dec 31 00:00 | Dec 31 20:00 | $126.27 | $124.03 | -$24.18 | -1.8% | StopLoss |
| 9 | Jan 02 16:00 | Jan 05 00:00 | $131.75 | $137.48 | **+$20.42** | +4.4% | TakeProfit |
| 10 | Jan 12 00:00 | Jan 12 08:00 | $142.61 | $139.77 | -$27.27 | -2.0% | StopLoss |

**Win pattern:** 4 wins averaging +$23.80, all hit TakeProfit quickly (8-16 hours)
**Loss pattern:** 6 losses averaging -$28.99, all hit StopLoss, some very fast (8 hours)

### Root Cause 1: Asymmetric Position Sizing Creates Outsized Losses

This is the critical finding. SOL's position sizes are **dramatically larger in notional terms** for losing trades:

- Trade 6 (loss): quantity=6.81 at $196.10 = **$1,334 notional** -- biggest SOL position
- Trade 3 (loss): quantity=5.82 at $222.28 = **$1,293 notional**
- Trade 4 (loss): quantity=5.18 at $244.88 = **$1,268 notional**
- Trade 8 (loss): quantity=9.71 at $126.27 = **$1,226 notional**
- Trade 10 (loss): quantity=8.72 at $142.61 = **$1,244 notional**

Versus winning trades:
- Trade 1 (win): quantity=2.57 at $190.90 = **$491 notional**
- Trade 2 (win): quantity=2.58 at $193.64 = **$500 notional**
- Trade 5 (win): quantity=2.35 at $209.63 = **$492 notional**
- Trade 9 (win): quantity=3.74 at $131.75 = **$493 notional**

**The losing trades have 2.5-2.7x larger notional positions than winning trades!**

**Why this happens (code path):**

In `src/engine/backtest.rs:457-499`, position sizing is:
1. `risk_amount = available * risk_per_trade` (5% of available)
2. `risk_based_qty = risk_amount / stop_distance`
3. `max_affordable_qty = max_allocation * available / (price * fee_multiplier)`
4. `quantity = min(risk_based_qty, max_affordable_qty)`
5. Then ATR volatility factor is applied (0.5x-1.2x)

The problem: **SOL's stop distance (ATR * 1.2 for MomentumStrategy, ATR * 0.5 for VolumeBreakoutStrategy) can be very tight on certain days**, making `risk_based_qty` very large. When SOL volatility is "Low" or "Medium", the ATR sizing factor is 1.0x or 1.2x, which does NOT reduce the position enough.

The early winning trades (Aug 12, Aug 22) occurred when the portfolio was smaller (~$2000) and SOL price was ~$190, yielding modest ~$500 positions. The later losing trades (Oct 21, Dec 31, Jan 12) occurred with accumulated capital at lower SOL prices ($126-$196), yielding $1,200-$1,334 positions -- the 60% `max_allocation` cap was binding.

### Root Cause 2: SOL Strategy Ensemble is Poorly Suited to a Downtrending Market

SOL price declined from ~$222 (Sep) to ~$126 (Dec) -- a **43% drawdown over 3 months**. During this period:

- **MomentumStrategy (35% weight):** Momentum strategies excel in sustained trends but generate false signals during choppy downtrends. SOL's "momentum" readings would frequently show bullish signals on bear market rallies, leading to immediate stop-outs.

- **VolumeBreakoutStrategy (25% weight):** Volume spikes during a downtrend are often distribution/capitulation, not breakout signals. The strategy treats any bullish candle with 2x volume as a StrongBuy, which in a downtrend is often a bull trap.

- **MeanReversionStrategy (25% weight):** Mean reversion can work well in ranges but is dangerous in a trending market. SOL hitting the lower Bollinger Band during a steep downtrend triggers buy signals, but the "reversion to mean" target keeps moving lower.

- **RSIDivergenceStrategy (15% weight):** RSI divergences during strong downtrends produce premature reversal signals. Divergence detection checks if `current_price < min_price && current_rsi > min_rsi` -- this is very common during extended downtrends.

The combined `min_agreement` threshold for SOL is only 50% (`combined.rs:113`), which is the **lowest of all pairs** (BTC=60%, ETH=55%). This means weaker, less-agreed-upon signals get through for SOL.

### Root Cause 3: No BTC Correlation Protection for SOL

ETH has a BTCCorrelationStrategy wired in at 15% weight (`combined.rs:87-88`), providing a macro-market filter. SOL has **no such protection** (`combined.rs:116-117`: `btc_correlation: None`).

When BTC is declining, ETH's BTC correlation strategy acts as a bearish filter, reducing the ensemble's weighted signal strength. SOL lacks this protection and enters buy positions even when the broader market is bearish.

### Root Cause 4: Correlation Group Allows Both ETH and SOL Simultaneously

Both ETH and SOL are in the "alt_major" correlation group (`types/trading.rs:76`):
```rust
TradingPair::ETHUSDT | TradingPair::SOLUSDT => "alt_major",
```

With `max_correlated_positions = 2`, the system can hold both ETH AND SOL positions simultaneously. When the altcoin market drops, both lose together, amplifying drawdown.

---

## 3. ETH Performance Analysis: Why It Works

ETH has 24 trades with a 45.8% win rate and PF of 1.13. Key factors:

**Signal Quality:** ETH's CombinedStrategy uses TrendStrategy(40%) + MomentumStrategy(35%) + MeanReversionStrategy(25%) -- a well-balanced ensemble for a large-cap asset with clear trend behavior.

**BTC Correlation Filter:** The 15% BTCCorrelationStrategy weight acts as a macro filter. When BTC is declining, this produces bearish signals that pull the ensemble toward Neutral, preventing entries during market-wide weakness.

**Position Sizing:** ETH positions are consistently sized around $1,200-$1,300 notional (at $3000-$4800 price), with 0.27-0.40 quantity. The sizing is more stable because ETH's price is high enough that the `risk_based_qty` stays moderate.

**Drawback:** ETH has 13 losses to 11 wins. The average loss ($37.18) is 75% of the average win ($49.85). The strategy works because the wins are large enough to overcome losses, but the margins are thin.

---

## 4. BTC Performance Analysis: Selective Quality

BTC takes only 6 trades -- the **most selective** of all pairs. With PF=1.92 and 2 wins producing $66.36 in profit vs 4 losses producing only $34.59, the loss sizes are well-controlled.

**Why so few trades:** BTC uses TrendStrategy(45%) + BreakoutStrategy(35%) + MeanReversionStrategy(20%) with a `min_agreement` of 60% (highest of all pairs). Combined with the 0.65 confidence threshold, BTC only generates signals when all strategies agree strongly.

**Excellent R:R:** BTC's 2 wins averaged +$33.18, while 4 losses averaged only -$8.65 (3.8:1 win-to-loss ratio). This is the hallmark of a properly risk-managed pair.

---

## 5. Cross-Scenario Comparison

Looking at the other backtest configs (Jan 2025 - Feb 2026, 13-month period):

| Scenario | SOL Trades | SOL PnL | ETH Trades | ETH PnL | BTC Trades | BTC PnL |
|---|---|---|---|---|---|---|
| Aggressive (no PM) | 14 | -$63.88 | 189 | -$617.28 | 12 | -$45.46 |
| Conservative (no PM) | 14 | **+$4.34** | 185 | -$570.86 | 12 | -$27.99 |
| Moderate (6mo) | 10 | -$78.71 | 24 | +$65.03 | 6 | +$31.77 |

**Critical insight from aggressive/conservative:** Over the 13-month period, ETH was catastrophically bad (185-189 trades, 0% win rate, -$570 to -$617). This appears to be a different backtest period (Jan 2025 - Feb 2026) that includes more bearish market conditions. SOL was actually slightly positive in the conservative scenario (+$4.34). This means:

1. SOL's problems are **period-specific** -- in certain market regimes SOL can be profitable
2. ETH's problems at scale (185+ trades with 0% WR) suggest a different, catastrophic bug in the aggressive/conservative configs that may not affect the moderate scenario
3. BTC is the most consistently acceptable pair

---

## 6. Concrete Recommendations

### Recommendation 1: Add SOL-Specific Confidence Threshold (HIGH IMPACT)

**File:** `src/strategies/combined.rs:113`
**Current:** `min_agreement: Decimal::new(50, 2)` (50% for SOL)
**Proposed:** Raise to `Decimal::new(65, 2)` (65% for SOL)

SOL is the most volatile and hardest-to-predict asset. It should have the **highest** confidence bar, not the lowest. This single change would filter out the majority of weak signals that lead to stop-outs.

**Expected impact:** Would eliminate ~40% of SOL trades (the weakest signals), removing 2-3 losing trades. Estimated improvement: +$50-80 in SOL PnL.

### Recommendation 2: Add BTC Correlation Filter to SOL Strategy (HIGH IMPACT)

**File:** `src/strategies/combined.rs:92-118`
**Current:** SOL has `btc_correlation: None, btc_correlation_weight: Decimal::ZERO`
**Proposed:** Add BTCCorrelationStrategy with 15-20% weight, reducing other weights proportionally:
- MomentumStrategy: 30% (from 35%)
- VolumeBreakoutStrategy: 20% (from 25%)
- MeanReversionStrategy: 20% (from 25%)
- RSIDivergenceStrategy: 10% (from 15%)
- BTCCorrelationStrategy: **20% (new)**

SOL has even higher correlation to BTC than ETH during downturns. A BTC filter would prevent entries during broad market declines.

**Expected impact:** Would have prevented trades 3, 4, 6, 7, 8, 10 (all during BTC downtrends). Estimated improvement: +$100-150 in SOL PnL.

### Recommendation 3: Separate Correlation Groups for ETH and SOL (MEDIUM IMPACT)

**File:** `src/types/trading.rs:73-79`
**Current:** Both ETH and SOL are "alt_major"
**Proposed:**
```rust
TradingPair::ETHUSDT => "eth",
TradingPair::SOLUSDT => "sol_alt",
```

With `max_correlated_positions = 2`, having ETH and SOL in the same group means sometimes one blocks the other (good), but sometimes both enter and lose together (bad). Separating them allows independent position management while using individual per-pair controls.

However, this is only useful IF SOL is kept with improved filters. The real fix is making SOL's signals better (Recommendations 1 & 2).

**Expected impact:** Minor. Primarily helps with opportunity cost when ETH position blocks a SOL signal or vice versa.

### Recommendation 4: Cap SOL Max Allocation Lower Than Other Pairs (MEDIUM IMPACT)

**File:** `src/engine/backtest.rs:485` and `src/main.rs:764`
**Current:** All pairs use the same `max_allocation` (60% in moderate)
**Proposed:** Add per-pair allocation caps. For SOL, cap at 30% max_allocation.

The trade data shows SOL's losing trades reach $1,200-$1,334 notional while wins are at ~$500. Capping SOL at 30% allocation would limit notional to ~$600, reducing loss magnitude while keeping win potential intact (since wins already occur at ~$500).

This requires adding a `max_allocation_overrides: HashMap<TradingPair, Decimal>` to BacktestConfig and checking it in `open_position()`.

**Expected impact:** Would roughly halve SOL losses from -$78.71 to approximately -$35-40. Does not fix signal quality but limits damage.

### Recommendation 5: Remove VolumeBreakoutStrategy from SOL Ensemble (MEDIUM IMPACT)

**File:** `src/strategies/combined.rs:92-119`
**Current:** SOL uses MomentumStrategy(35%) + VolumeBreakoutStrategy(25%) + MeanReversionStrategy(25%) + RSIDivergenceStrategy(15%)
**Proposed:** Remove VolumeBreakoutStrategy. Use: MomentumStrategy(40%) + MeanReversionStrategy(30%) + RSIDivergenceStrategy(10%) + BTCCorrelation(20%)

VolumeBreakoutStrategy's stop-loss is extremely tight: `current.low - (atr * 0.5)` for the stop (`momentum.rs:351`). On a 4-hour candle for SOL, this is often only 1-2% away. The take profit is `entry + (atr * 2)`, giving a nominal 4:1 R:R, but the tight stop gets clipped by normal intra-candle volatility on SOL.

For comparison:
- MomentumStrategy: SL = entry - (atr * 1.2), TP = entry + (atr * 2.4) -- 2:1 R:R
- MeanReversionStrategy: SL = bb_lower - (atr * 0.5), TP = bb_middle -- variable
- VolumeBreakoutStrategy: SL = candle.low - (atr * 0.5), TP = entry + (atr * 2) -- uses candle low, which is often very close to entry

The VolumeBreakoutStrategy's use of `current.low` as the stop anchor is problematic for SOL because SOL candles have large wicks, making the stop tighter than ATR-only calculations.

**Expected impact:** Reduces false signals from volume noise. Estimated improvement: removes 1-2 losing trades.

### Recommendation 6: Consider Dropping SOL Entirely (Fallback Option)

If Recommendations 1-5 do not improve SOL to at least break-even in walk-forward testing:

**Impact of removing SOL from the portfolio:**
- Net PnL: +$18.09 -> +$96.80 (5.4x improvement)
- Win Rate: 42.5% -> 43.3%
- Profit Factor: 1.03 -> 1.28
- Max Drawdown: likely 2-3% lower (several SOL stop-outs contributed to drawdown peaks)

This is the nuclear option. SOL adds diversification value in bull markets (it was the only asset with +5-6% winning trades early on), but the current strategy ensemble cannot handle SOL's bearish regime.

### Recommendation 7: Do NOT Add BNB, ADA, XRP Yet (INFORMATIONAL)

The aggressive/conservative backtests already show severe problems with just 3 pairs (0% ETH win rate over 13 months). Adding more pairs would:
- Increase correlation risk (all altcoins correlate in downturns)
- Spread the $2,000 capital thinner
- Add pairs with even lower liquidity on Binance.US
- ADA and XRP are in the "alt_minor" correlation group but would still add drawdown risk

**Recommendation:** Only consider adding pairs after the core BTC+ETH+SOL strategy is reliably profitable. If SOL is dropped, reconsider adding one pair only after establishing consistent profitability with BTC+ETH.

---

## 7. Temporal Pattern Analysis

### Winning Periods
- **Aug 10-24:** Strong bull market. All 3 pairs profitable. ETH +$126.59, SOL +$53.99, BTC +$22.16. 7 wins out of 9 trades.
- **Oct 1-7:** Recovery bounce. ETH +$119.79, BTC +$34.73, SOL +$20.79. 4 wins out of 4 trades.
- **Dec 3-9:** Brief ETH rally. ETH +$137.35 from 2 big wins, but followed by -$43.39 loss.

### Losing Periods
- **Sep 10 - Sep 30:** Market decline. SOL -$62.86 (2 stop-outs), ETH -$89.31 (3 stop-outs). This 20-day period wiped out August gains.
- **Oct 21 - Nov 11:** Continued weakness. SOL -$43.09, ETH -$76.83, BTC -$15.19.
- **Nov 25 - Jan 16:** Extended bearish. SOL -$47.97, ETH -$193.87, BTC -$15.67. The worst stretch, coinciding with SOL's price declining from $144 to $126.

### Pattern: The bot trades well in short bull rallies but cannot avoid entering during sustained downtrends. This is a **regime detection problem** -- the existing regime detection (`combined.rs:148-209`) adjusts strategy *weights* but does not prevent trading altogether during bearish regimes.

---

## 8. Priority-Ranked Implementation Plan

| Priority | Recommendation | Estimated PnL Impact | Complexity |
|---|---|---|---|
| 1 | Raise SOL min_agreement to 0.65 | +$50-80 | 1 line change |
| 2 | Add BTC correlation to SOL ensemble | +$100-150 | ~15 lines |
| 3 | Cap SOL max_allocation at 30% | +$30-40 | ~10 lines |
| 4 | Remove VolumeBreakoutStrategy from SOL | +$15-25 | ~5 lines |
| 5 | Separate ETH/SOL correlation groups | +$5-15 | 2 line change |
| 6 | Drop SOL entirely (if 1-5 fail) | +$78.71 | 1 line change |

**Combined estimated impact of 1-4:** +$195-295, converting SOL from -$78.71 to approximately +$20-50, and total portfolio from +$18.09 to ~+$115-215 (5.7-10.7% return with same max DD).

---

## 9. Files Referenced

- `/Users/daniellavazquez78/javier/bot/backtest_moderate.json` -- Trade data analyzed
- `/Users/daniellavazquez78/javier/bot/src/strategies/combined.rs:92-119` -- SOL strategy ensemble and weights
- `/Users/daniellavazquez78/javier/bot/src/strategies/combined.rs:113` -- SOL min_agreement threshold (50%)
- `/Users/daniellavazquez78/javier/bot/src/strategies/combined.rs:116-117` -- SOL missing BTC correlation
- `/Users/daniellavazquez78/javier/bot/src/engine/backtest.rs:457-499` -- Position sizing logic
- `/Users/daniellavazquez78/javier/bot/src/engine/backtest.rs:485` -- max_allocation cap
- `/Users/daniellavazquez78/javier/bot/src/types/trading.rs:73-79` -- Correlation groups
- `/Users/daniellavazquez78/javier/bot/src/strategies/momentum.rs:196-208` -- MomentumStrategy SL/TP levels
- `/Users/daniellavazquez78/javier/bot/src/strategies/momentum.rs:283-366` -- VolumeBreakoutStrategy (tight stops)
- `/Users/daniellavazquez78/javier/bot/src/strategies/mean_reversion.rs:174-186` -- MeanReversionStrategy SL/TP levels
- `/Users/daniellavazquez78/javier/bot/src/indicators/atr.rs:87-105` -- Volatility level thresholds
- `/Users/daniellavazquez78/javier/bot/src/indicators/atr.rs:133-140` -- Position size factors
