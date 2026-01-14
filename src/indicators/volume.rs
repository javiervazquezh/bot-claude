use rust_decimal::Decimal;
use super::{Indicator, sma};

#[derive(Debug, Clone)]
pub struct VolumeProfile {
    period: usize,
    volumes: Vec<Decimal>,
    avg_volume: Option<Decimal>,
}

impl VolumeProfile {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            volumes: Vec::with_capacity(period),
            avg_volume: None,
        }
    }

    pub fn update(&mut self, volume: Decimal) -> Option<Decimal> {
        self.volumes.push(volume);
        if self.volumes.len() > self.period {
            self.volumes.remove(0);
        }

        if self.volumes.len() >= self.period {
            self.avg_volume = sma(&self.volumes, self.period);
        }

        self.avg_volume
    }

    pub fn avg_volume(&self) -> Option<Decimal> {
        self.avg_volume
    }

    pub fn relative_volume(&self, current_volume: Decimal) -> Option<Decimal> {
        self.avg_volume.map(|avg| {
            if avg.is_zero() {
                Decimal::ONE
            } else {
                current_volume / avg
            }
        })
    }

    pub fn is_high_volume(&self, current_volume: Decimal, threshold: Decimal) -> bool {
        self.relative_volume(current_volume)
            .map(|rv| rv > threshold)
            .unwrap_or(false)
    }

    pub fn is_low_volume(&self, current_volume: Decimal, threshold: Decimal) -> bool {
        self.relative_volume(current_volume)
            .map(|rv| rv < threshold)
            .unwrap_or(false)
    }
}

impl Indicator for VolumeProfile {
    fn name(&self) -> &'static str {
        "VolumeProfile"
    }

    fn is_ready(&self) -> bool {
        self.avg_volume.is_some()
    }

    fn reset(&mut self) {
        self.volumes.clear();
        self.avg_volume = None;
    }
}

#[derive(Debug, Clone)]
pub struct OBV {
    value: Decimal,
    prev_close: Option<Decimal>,
}

impl OBV {
    pub fn new() -> Self {
        Self {
            value: Decimal::ZERO,
            prev_close: None,
        }
    }

    pub fn update(&mut self, close: Decimal, volume: Decimal) -> Decimal {
        if let Some(prev) = self.prev_close {
            if close > prev {
                self.value += volume;
            } else if close < prev {
                self.value -= volume;
            }
        }
        self.prev_close = Some(close);
        self.value
    }

    pub fn value(&self) -> Decimal {
        self.value
    }
}

impl Default for OBV {
    fn default() -> Self {
        Self::new()
    }
}

impl Indicator for OBV {
    fn name(&self) -> &'static str {
        "OBV"
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some()
    }

    fn reset(&mut self) {
        self.value = Decimal::ZERO;
        self.prev_close = None;
    }
}

#[derive(Debug, Clone)]
pub struct VWAP {
    cumulative_tp_volume: Decimal,
    cumulative_volume: Decimal,
    value: Option<Decimal>,
}

impl VWAP {
    pub fn new() -> Self {
        Self {
            cumulative_tp_volume: Decimal::ZERO,
            cumulative_volume: Decimal::ZERO,
            value: None,
        }
    }

    pub fn update(&mut self, high: Decimal, low: Decimal, close: Decimal, volume: Decimal) -> Option<Decimal> {
        let typical_price = (high + low + close) / Decimal::from(3);
        self.cumulative_tp_volume += typical_price * volume;
        self.cumulative_volume += volume;

        if !self.cumulative_volume.is_zero() {
            self.value = Some(self.cumulative_tp_volume / self.cumulative_volume);
        }

        self.value
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn price_vs_vwap(&self, price: Decimal) -> Option<PriceVsVWAP> {
        self.value.map(|vwap| {
            let diff_pct = if !vwap.is_zero() {
                ((price - vwap) / vwap) * Decimal::from(100)
            } else {
                Decimal::ZERO
            };

            if diff_pct > Decimal::from(2) {
                PriceVsVWAP::WellAbove
            } else if diff_pct > Decimal::ZERO {
                PriceVsVWAP::Above
            } else if diff_pct < Decimal::from(-2) {
                PriceVsVWAP::WellBelow
            } else if diff_pct < Decimal::ZERO {
                PriceVsVWAP::Below
            } else {
                PriceVsVWAP::AtVWAP
            }
        })
    }
}

impl Default for VWAP {
    fn default() -> Self {
        Self::new()
    }
}

impl Indicator for VWAP {
    fn name(&self) -> &'static str {
        "VWAP"
    }

    fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    fn reset(&mut self) {
        self.cumulative_tp_volume = Decimal::ZERO;
        self.cumulative_volume = Decimal::ZERO;
        self.value = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceVsVWAP {
    WellAbove,
    Above,
    AtVWAP,
    Below,
    WellBelow,
}

impl PriceVsVWAP {
    pub fn is_bullish(&self) -> bool {
        matches!(self, PriceVsVWAP::WellAbove | PriceVsVWAP::Above)
    }

    pub fn is_bearish(&self) -> bool {
        matches!(self, PriceVsVWAP::WellBelow | PriceVsVWAP::Below)
    }
}

#[derive(Debug, Clone)]
pub struct MoneyFlowIndex {
    period: usize,
    prev_typical_price: Option<Decimal>,
    positive_flows: Vec<Decimal>,
    negative_flows: Vec<Decimal>,
    value: Option<Decimal>,
}

impl MoneyFlowIndex {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prev_typical_price: None,
            positive_flows: Vec::with_capacity(period),
            negative_flows: Vec::with_capacity(period),
            value: None,
        }
    }

    pub fn update(&mut self, high: Decimal, low: Decimal, close: Decimal, volume: Decimal) -> Option<Decimal> {
        let typical_price = (high + low + close) / Decimal::from(3);
        let raw_money_flow = typical_price * volume;

        if let Some(prev_tp) = self.prev_typical_price {
            if typical_price > prev_tp {
                self.positive_flows.push(raw_money_flow);
                self.negative_flows.push(Decimal::ZERO);
            } else if typical_price < prev_tp {
                self.positive_flows.push(Decimal::ZERO);
                self.negative_flows.push(raw_money_flow);
            } else {
                self.positive_flows.push(Decimal::ZERO);
                self.negative_flows.push(Decimal::ZERO);
            }

            if self.positive_flows.len() > self.period {
                self.positive_flows.remove(0);
                self.negative_flows.remove(0);
            }

            if self.positive_flows.len() >= self.period {
                let pos_sum: Decimal = self.positive_flows.iter().sum();
                let neg_sum: Decimal = self.negative_flows.iter().sum();

                if neg_sum.is_zero() {
                    self.value = Some(Decimal::from(100));
                } else {
                    let money_ratio = pos_sum / neg_sum;
                    self.value = Some(Decimal::from(100) - (Decimal::from(100) / (Decimal::ONE + money_ratio)));
                }
            }
        }

        self.prev_typical_price = Some(typical_price);
        self.value
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn is_oversold(&self) -> bool {
        self.value.map(|v| v < Decimal::from(20)).unwrap_or(false)
    }

    pub fn is_overbought(&self) -> bool {
        self.value.map(|v| v > Decimal::from(80)).unwrap_or(false)
    }
}

impl Indicator for MoneyFlowIndex {
    fn name(&self) -> &'static str {
        "MFI"
    }

    fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    fn reset(&mut self) {
        self.prev_typical_price = None;
        self.positive_flows.clear();
        self.negative_flows.clear();
        self.value = None;
    }
}
