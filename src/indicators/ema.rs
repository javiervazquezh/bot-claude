use rust_decimal::Decimal;
use super::Indicator;

#[derive(Debug, Clone)]
pub struct EMA {
    period: usize,
    multiplier: Decimal,
    value: Option<Decimal>,
    count: usize,
    sum: Decimal,
}

impl EMA {
    pub fn new(period: usize) -> Self {
        let multiplier = Decimal::from(2) / Decimal::from(period as u32 + 1);
        Self {
            period,
            multiplier,
            value: None,
            count: 0,
            sum: Decimal::ZERO,
        }
    }

    pub fn update(&mut self, price: Decimal) -> Option<Decimal> {
        self.count += 1;

        if self.count < self.period {
            self.sum += price;
            return None;
        } else if self.count == self.period {
            self.sum += price;
            let sma = self.sum / Decimal::from(self.period as u32);
            self.value = Some(sma);
            return self.value;
        }

        if let Some(prev_ema) = self.value {
            let new_ema = (price - prev_ema) * self.multiplier + prev_ema;
            self.value = Some(new_ema);
        }

        self.value
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn period(&self) -> usize {
        self.period
    }
}

impl Indicator for EMA {
    fn name(&self) -> &'static str {
        "EMA"
    }

    fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    fn reset(&mut self) {
        self.value = None;
        self.count = 0;
        self.sum = Decimal::ZERO;
    }
}

pub fn calculate_ema_series(prices: &[Decimal], period: usize) -> Vec<Decimal> {
    let mut ema = EMA::new(period);
    prices
        .iter()
        .filter_map(|p| ema.update(*p))
        .collect()
}

#[derive(Debug, Clone)]
pub struct DoubleEMA {
    fast: EMA,
    slow: EMA,
}

impl DoubleEMA {
    pub fn new(fast_period: usize, slow_period: usize) -> Self {
        Self {
            fast: EMA::new(fast_period),
            slow: EMA::new(slow_period),
        }
    }

    pub fn update(&mut self, price: Decimal) -> (Option<Decimal>, Option<Decimal>) {
        let fast = self.fast.update(price);
        let slow = self.slow.update(price);
        (fast, slow)
    }

    pub fn fast_value(&self) -> Option<Decimal> {
        self.fast.value()
    }

    pub fn slow_value(&self) -> Option<Decimal> {
        self.slow.value()
    }

    pub fn crossover(&self) -> Option<bool> {
        match (self.fast.value(), self.slow.value()) {
            (Some(fast), Some(slow)) => Some(fast > slow),
            _ => None,
        }
    }

    pub fn spread(&self) -> Option<Decimal> {
        match (self.fast.value(), self.slow.value()) {
            (Some(fast), Some(slow)) => Some(fast - slow),
            _ => None,
        }
    }

    pub fn spread_percentage(&self) -> Option<Decimal> {
        match (self.fast.value(), self.slow.value()) {
            (Some(fast), Some(slow)) if !slow.is_zero() => {
                Some(((fast - slow) / slow) * Decimal::from(100))
            }
            _ => None,
        }
    }
}

impl Indicator for DoubleEMA {
    fn name(&self) -> &'static str {
        "DoubleEMA"
    }

    fn is_ready(&self) -> bool {
        self.fast.is_ready() && self.slow.is_ready()
    }

    fn reset(&mut self) {
        self.fast.reset();
        self.slow.reset();
    }
}
