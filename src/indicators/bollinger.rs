#![allow(dead_code)]
use rust_decimal::Decimal;
use super::{Indicator, stddev, sma};

#[derive(Debug, Clone)]
pub struct BollingerBands {
    period: usize,
    std_dev_multiplier: Decimal,
    prices: Vec<Decimal>,
    upper: Option<Decimal>,
    middle: Option<Decimal>,
    lower: Option<Decimal>,
    bandwidth: Option<Decimal>,
    percent_b: Option<Decimal>,
}

impl BollingerBands {
    pub fn new(period: usize, std_dev_multiplier: Decimal) -> Self {
        Self {
            period,
            std_dev_multiplier,
            prices: Vec::with_capacity(period),
            upper: None,
            middle: None,
            lower: None,
            bandwidth: None,
            percent_b: None,
        }
    }

    pub fn default_params() -> Self {
        Self::new(20, Decimal::from(2))
    }

    pub fn update(&mut self, price: Decimal) -> Option<BollingerOutput> {
        self.prices.push(price);
        if self.prices.len() > self.period {
            self.prices.remove(0);
        }

        if self.prices.len() < self.period {
            return None;
        }

        let middle = sma(&self.prices, self.period)?;
        let std_dev = stddev(&self.prices, self.period)?;

        let deviation = std_dev * self.std_dev_multiplier;
        let upper = middle + deviation;
        let lower = middle - deviation;

        self.upper = Some(upper);
        self.middle = Some(middle);
        self.lower = Some(lower);

        // Calculate bandwidth: (upper - lower) / middle * 100
        self.bandwidth = if !middle.is_zero() {
            Some((upper - lower) / middle * Decimal::from(100))
        } else {
            None
        };

        // Calculate %B: (price - lower) / (upper - lower)
        let band_range = upper - lower;
        self.percent_b = if !band_range.is_zero() {
            Some((price - lower) / band_range)
        } else {
            None
        };

        Some(BollingerOutput {
            upper,
            middle,
            lower,
            bandwidth: self.bandwidth?,
            percent_b: self.percent_b?,
        })
    }

    pub fn upper(&self) -> Option<Decimal> {
        self.upper
    }

    pub fn middle(&self) -> Option<Decimal> {
        self.middle
    }

    pub fn lower(&self) -> Option<Decimal> {
        self.lower
    }

    pub fn bandwidth(&self) -> Option<Decimal> {
        self.bandwidth
    }

    pub fn percent_b(&self) -> Option<Decimal> {
        self.percent_b
    }

    pub fn is_squeeze(&self, threshold: Decimal) -> bool {
        self.bandwidth.map(|bw| bw < threshold).unwrap_or(false)
    }

    pub fn is_expansion(&self, threshold: Decimal) -> bool {
        self.bandwidth.map(|bw| bw > threshold).unwrap_or(false)
    }

    pub fn position(&self, price: Decimal) -> Option<BollingerPosition> {
        match (self.upper, self.lower, self.percent_b) {
            (Some(upper), Some(lower), Some(pct_b)) => {
                if price >= upper {
                    Some(BollingerPosition::AboveUpper)
                } else if price <= lower {
                    Some(BollingerPosition::BelowLower)
                } else if pct_b > Decimal::new(8, 1) {
                    Some(BollingerPosition::UpperHalf)
                } else if pct_b < Decimal::new(2, 1) {
                    Some(BollingerPosition::LowerHalf)
                } else {
                    Some(BollingerPosition::Middle)
                }
            }
            _ => None,
        }
    }
}

impl Indicator for BollingerBands {
    fn name(&self) -> &'static str {
        "BollingerBands"
    }

    fn is_ready(&self) -> bool {
        self.middle.is_some()
    }

    fn reset(&mut self) {
        self.prices.clear();
        self.upper = None;
        self.middle = None;
        self.lower = None;
        self.bandwidth = None;
        self.percent_b = None;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BollingerOutput {
    pub upper: Decimal,
    pub middle: Decimal,
    pub lower: Decimal,
    pub bandwidth: Decimal,
    pub percent_b: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BollingerPosition {
    AboveUpper,
    UpperHalf,
    Middle,
    LowerHalf,
    BelowLower,
}

impl BollingerPosition {
    pub fn is_extreme(&self) -> bool {
        matches!(self, BollingerPosition::AboveUpper | BollingerPosition::BelowLower)
    }

    pub fn is_overbought(&self) -> bool {
        matches!(self, BollingerPosition::AboveUpper | BollingerPosition::UpperHalf)
    }

    pub fn is_oversold(&self) -> bool {
        matches!(self, BollingerPosition::BelowLower | BollingerPosition::LowerHalf)
    }
}
