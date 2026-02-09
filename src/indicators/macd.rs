#![allow(dead_code)]
use rust_decimal::Decimal;
use super::{ema::EMA, Indicator};

#[derive(Debug, Clone)]
pub struct MACD {
    fast_ema: EMA,
    slow_ema: EMA,
    signal_ema: EMA,
    macd_line: Option<Decimal>,
    signal_line: Option<Decimal>,
    histogram: Option<Decimal>,
    prev_histogram: Option<Decimal>,
}

impl MACD {
    pub fn new(fast_period: usize, slow_period: usize, signal_period: usize) -> Self {
        Self {
            fast_ema: EMA::new(fast_period),
            slow_ema: EMA::new(slow_period),
            signal_ema: EMA::new(signal_period),
            macd_line: None,
            signal_line: None,
            histogram: None,
            prev_histogram: None,
        }
    }

    pub fn default_params() -> Self {
        Self::new(12, 26, 9)
    }

    pub fn update(&mut self, price: Decimal) -> Option<MACDOutput> {
        let fast = self.fast_ema.update(price);
        let slow = self.slow_ema.update(price);

        match (fast, slow) {
            (Some(f), Some(s)) => {
                let macd_line = f - s;
                self.macd_line = Some(macd_line);

                if let Some(signal) = self.signal_ema.update(macd_line) {
                    self.prev_histogram = self.histogram;
                    let histogram = macd_line - signal;
                    self.signal_line = Some(signal);
                    self.histogram = Some(histogram);

                    return Some(MACDOutput {
                        macd_line,
                        signal_line: signal,
                        histogram,
                    });
                }
            }
            _ => {}
        }

        None
    }

    pub fn macd_line(&self) -> Option<Decimal> {
        self.macd_line
    }

    pub fn signal_line(&self) -> Option<Decimal> {
        self.signal_line
    }

    pub fn histogram(&self) -> Option<Decimal> {
        self.histogram
    }

    pub fn is_bullish_crossover(&self) -> bool {
        match (self.histogram, self.prev_histogram) {
            (Some(curr), Some(prev)) => prev < Decimal::ZERO && curr >= Decimal::ZERO,
            _ => false,
        }
    }

    pub fn is_bearish_crossover(&self) -> bool {
        match (self.histogram, self.prev_histogram) {
            (Some(curr), Some(prev)) => prev > Decimal::ZERO && curr <= Decimal::ZERO,
            _ => false,
        }
    }

    pub fn histogram_increasing(&self) -> bool {
        match (self.histogram, self.prev_histogram) {
            (Some(curr), Some(prev)) => curr > prev,
            _ => false,
        }
    }

    pub fn histogram_decreasing(&self) -> bool {
        match (self.histogram, self.prev_histogram) {
            (Some(curr), Some(prev)) => curr < prev,
            _ => false,
        }
    }

    pub fn trend(&self) -> Option<MACDTrend> {
        match (self.macd_line, self.signal_line, self.histogram) {
            (Some(macd), Some(signal), Some(hist)) => {
                if macd > Decimal::ZERO && signal > Decimal::ZERO && hist > Decimal::ZERO {
                    Some(MACDTrend::StrongBullish)
                } else if macd > Decimal::ZERO && hist > Decimal::ZERO {
                    Some(MACDTrend::Bullish)
                } else if macd < Decimal::ZERO && signal < Decimal::ZERO && hist < Decimal::ZERO {
                    Some(MACDTrend::StrongBearish)
                } else if macd < Decimal::ZERO && hist < Decimal::ZERO {
                    Some(MACDTrend::Bearish)
                } else {
                    Some(MACDTrend::Neutral)
                }
            }
            _ => None,
        }
    }
}

impl Indicator for MACD {
    fn name(&self) -> &'static str {
        "MACD"
    }

    fn is_ready(&self) -> bool {
        self.histogram.is_some()
    }

    fn reset(&mut self) {
        self.fast_ema.reset();
        self.slow_ema.reset();
        self.signal_ema.reset();
        self.macd_line = None;
        self.signal_line = None;
        self.histogram = None;
        self.prev_histogram = None;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MACDOutput {
    pub macd_line: Decimal,
    pub signal_line: Decimal,
    pub histogram: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MACDTrend {
    StrongBullish,
    Bullish,
    Neutral,
    Bearish,
    StrongBearish,
}

impl MACDTrend {
    pub fn is_bullish(&self) -> bool {
        matches!(self, MACDTrend::StrongBullish | MACDTrend::Bullish)
    }

    pub fn is_bearish(&self) -> bool {
        matches!(self, MACDTrend::StrongBearish | MACDTrend::Bearish)
    }

    pub fn strength(&self) -> i8 {
        match self {
            MACDTrend::StrongBullish => 2,
            MACDTrend::Bullish => 1,
            MACDTrend::Neutral => 0,
            MACDTrend::Bearish => -1,
            MACDTrend::StrongBearish => -2,
        }
    }
}
