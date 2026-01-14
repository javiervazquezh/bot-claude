use rust_decimal::Decimal;
use super::Indicator;

#[derive(Debug, Clone)]
pub struct ATR {
    period: usize,
    prev_close: Option<Decimal>,
    true_ranges: Vec<Decimal>,
    value: Option<Decimal>,
}

impl ATR {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prev_close: None,
            true_ranges: Vec::with_capacity(period),
            value: None,
        }
    }

    pub fn update(&mut self, high: Decimal, low: Decimal, close: Decimal) -> Option<Decimal> {
        let tr = self.calculate_true_range(high, low, close);
        self.prev_close = Some(close);

        self.true_ranges.push(tr);

        if self.true_ranges.len() < self.period {
            return None;
        }

        if self.true_ranges.len() == self.period && self.value.is_none() {
            let sum: Decimal = self.true_ranges.iter().sum();
            self.value = Some(sum / Decimal::from(self.period as u32));
        } else if let Some(prev_atr) = self.value {
            let period_dec = Decimal::from(self.period as u32);
            let new_atr = (prev_atr * (period_dec - Decimal::ONE) + tr) / period_dec;
            self.value = Some(new_atr);
        }

        if self.true_ranges.len() > self.period {
            self.true_ranges.remove(0);
        }

        self.value
    }

    fn calculate_true_range(&self, high: Decimal, low: Decimal, close: Decimal) -> Decimal {
        let hl = high - low;

        match self.prev_close {
            Some(prev_close) => {
                let hc = (high - prev_close).abs();
                let lc = (low - prev_close).abs();
                hl.max(hc).max(lc)
            }
            None => hl,
        }
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn calculate_stop_loss(&self, entry_price: Decimal, multiplier: Decimal, is_long: bool) -> Option<Decimal> {
        self.value.map(|atr| {
            let distance = atr * multiplier;
            if is_long {
                entry_price - distance
            } else {
                entry_price + distance
            }
        })
    }

    pub fn calculate_take_profit(&self, entry_price: Decimal, multiplier: Decimal, is_long: bool) -> Option<Decimal> {
        self.value.map(|atr| {
            let distance = atr * multiplier;
            if is_long {
                entry_price + distance
            } else {
                entry_price - distance
            }
        })
    }

    pub fn volatility_level(&self, price: Decimal) -> Option<VolatilityLevel> {
        self.value.map(|atr| {
            let atr_pct = if !price.is_zero() {
                (atr / price) * Decimal::from(100)
            } else {
                return VolatilityLevel::Low;
            };

            if atr_pct > Decimal::from(5) {
                VolatilityLevel::Extreme
            } else if atr_pct > Decimal::from(3) {
                VolatilityLevel::High
            } else if atr_pct > Decimal::from(1) {
                VolatilityLevel::Medium
            } else {
                VolatilityLevel::Low
            }
        })
    }
}

impl Indicator for ATR {
    fn name(&self) -> &'static str {
        "ATR"
    }

    fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.true_ranges.clear();
        self.value = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolatilityLevel {
    Low,
    Medium,
    High,
    Extreme,
}

impl VolatilityLevel {
    pub fn position_size_factor(&self) -> Decimal {
        match self {
            VolatilityLevel::Low => Decimal::new(12, 1),    // 1.2x
            VolatilityLevel::Medium => Decimal::ONE,        // 1.0x
            VolatilityLevel::High => Decimal::new(7, 1),    // 0.7x
            VolatilityLevel::Extreme => Decimal::new(5, 1), // 0.5x
        }
    }
}

#[derive(Debug, Clone)]
pub struct ATRTrailingStop {
    atr: ATR,
    multiplier: Decimal,
    stop_price: Option<Decimal>,
    is_long: bool,
}

impl ATRTrailingStop {
    pub fn new(period: usize, multiplier: Decimal, is_long: bool) -> Self {
        Self {
            atr: ATR::new(period),
            multiplier,
            stop_price: None,
            is_long,
        }
    }

    pub fn update(&mut self, high: Decimal, low: Decimal, close: Decimal) -> Option<Decimal> {
        self.atr.update(high, low, close)?;

        let new_stop = self.atr.calculate_stop_loss(close, self.multiplier, self.is_long)?;

        self.stop_price = Some(match self.stop_price {
            Some(current) => {
                if self.is_long {
                    current.max(new_stop)
                } else {
                    current.min(new_stop)
                }
            }
            None => new_stop,
        });

        self.stop_price
    }

    pub fn is_stopped(&self, price: Decimal) -> bool {
        match self.stop_price {
            Some(stop) => {
                if self.is_long {
                    price <= stop
                } else {
                    price >= stop
                }
            }
            None => false,
        }
    }

    pub fn stop_price(&self) -> Option<Decimal> {
        self.stop_price
    }
}
