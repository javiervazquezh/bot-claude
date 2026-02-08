use rust_decimal::Decimal;
use super::Indicator;

#[derive(Debug, Clone)]
pub struct RSI {
    period: usize,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    prev_price: Option<Decimal>,
    gains: Vec<Decimal>,
    losses: Vec<Decimal>,
    value: Option<Decimal>,
}

impl RSI {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            avg_gain: None,
            avg_loss: None,
            prev_price: None,
            gains: Vec::with_capacity(period),
            losses: Vec::with_capacity(period),
            value: None,
        }
    }

    pub fn update(&mut self, price: Decimal) -> Option<Decimal> {
        if let Some(prev) = self.prev_price {
            let change = price - prev;
            let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
            let loss = if change < Decimal::ZERO { change.abs() } else { Decimal::ZERO };

            if self.gains.len() < self.period {
                self.gains.push(gain);
                self.losses.push(loss);

                if self.gains.len() == self.period {
                    let sum_gain: Decimal = self.gains.iter().sum();
                    let sum_loss: Decimal = self.losses.iter().sum();
                    self.avg_gain = Some(sum_gain / Decimal::from(self.period as u32));
                    self.avg_loss = Some(sum_loss / Decimal::from(self.period as u32));
                    self.value = self.calculate_rsi();
                }
            } else if let (Some(avg_gain), Some(avg_loss)) = (self.avg_gain, self.avg_loss) {
                let period_dec = Decimal::from(self.period as u32);
                let new_avg_gain = (avg_gain * (period_dec - Decimal::ONE) + gain) / period_dec;
                let new_avg_loss = (avg_loss * (period_dec - Decimal::ONE) + loss) / period_dec;
                self.avg_gain = Some(new_avg_gain);
                self.avg_loss = Some(new_avg_loss);
                self.value = self.calculate_rsi();
            }
        }

        self.prev_price = Some(price);
        self.value
    }

    fn calculate_rsi(&self) -> Option<Decimal> {
        match (self.avg_gain, self.avg_loss) {
            (Some(avg_gain), Some(avg_loss)) => {
                if avg_loss.is_zero() {
                    Some(Decimal::from(100))
                } else {
                    let rs = avg_gain / avg_loss;
                    Some(Decimal::from(100) - (Decimal::from(100) / (Decimal::ONE + rs)))
                }
            }
            _ => None,
        }
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn is_oversold(&self, threshold: Decimal) -> bool {
        self.value.map(|v| v < threshold).unwrap_or(false)
    }

    pub fn is_overbought(&self, threshold: Decimal) -> bool {
        self.value.map(|v| v > threshold).unwrap_or(false)
    }

    pub fn zone(&self) -> Option<RSIZone> {
        self.value.map(|v| {
            if v < Decimal::from(30) {
                RSIZone::Oversold
            } else if v > Decimal::from(70) {
                RSIZone::Overbought
            } else if v < Decimal::from(50) {
                RSIZone::BearishNeutral
            } else {
                RSIZone::BullishNeutral
            }
        })
    }
}

impl Indicator for RSI {
    fn name(&self) -> &'static str {
        "RSI"
    }

    fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    fn reset(&mut self) {
        self.avg_gain = None;
        self.avg_loss = None;
        self.prev_price = None;
        self.gains.clear();
        self.losses.clear();
        self.value = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RSIZone {
    Oversold,
    BearishNeutral,
    BullishNeutral,
    Overbought,
}

impl RSIZone {
    pub fn is_extreme(&self) -> bool {
        matches!(self, RSIZone::Oversold | RSIZone::Overbought)
    }
}

#[derive(Debug, Clone)]
pub struct StochasticRSI {
    rsi: RSI,
    period: usize,
    rsi_values: Vec<Decimal>,
    k_period: usize,
    d_period: usize,
    k_values: Vec<Decimal>,
}

impl StochasticRSI {
    pub fn new(rsi_period: usize, stoch_period: usize, k_period: usize, d_period: usize) -> Self {
        Self {
            rsi: RSI::new(rsi_period),
            period: stoch_period,
            rsi_values: Vec::with_capacity(stoch_period),
            k_period,
            d_period,
            k_values: Vec::with_capacity(d_period),
        }
    }

    pub fn update(&mut self, price: Decimal) -> Option<(Decimal, Decimal)> {
        let rsi_value = self.rsi.update(price)?;

        self.rsi_values.push(rsi_value);
        if self.rsi_values.len() > self.period {
            self.rsi_values.remove(0);
        }

        if self.rsi_values.len() < self.period {
            return None;
        }

        let highest = self.rsi_values.iter().max().copied()?;
        let lowest = self.rsi_values.iter().min().copied()?;

        let range = highest - lowest;
        let k = if range.is_zero() {
            Decimal::from(50)
        } else {
            ((rsi_value - lowest) / range) * Decimal::from(100)
        };

        self.k_values.push(k);
        if self.k_values.len() > self.d_period {
            self.k_values.remove(0);
        }

        if self.k_values.len() < self.d_period {
            return None;
        }

        let d: Decimal = self.k_values.iter().sum::<Decimal>() / Decimal::from(self.d_period as u32);

        Some((k, d))
    }
}

impl Indicator for StochasticRSI {
    fn name(&self) -> &'static str {
        "StochasticRSI"
    }

    fn is_ready(&self) -> bool {
        self.k_values.len() >= self.d_period
    }

    fn reset(&mut self) {
        self.rsi.reset();
        self.rsi_values.clear();
        self.k_values.clear();
    }
}
