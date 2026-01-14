pub mod ema;
pub mod rsi;
pub mod macd;
pub mod bollinger;
pub mod atr;
pub mod volume;

pub use ema::*;
pub use rsi::*;
pub use macd::*;
pub use bollinger::*;
pub use atr::*;
pub use volume::*;

use rust_decimal::Decimal;

pub trait Indicator {
    fn name(&self) -> &'static str;
    fn is_ready(&self) -> bool;
    fn reset(&mut self);
}

pub fn sma(values: &[Decimal], period: usize) -> Option<Decimal> {
    if values.len() < period {
        return None;
    }
    let sum: Decimal = values.iter().rev().take(period).sum();
    Some(sum / Decimal::from(period as u32))
}

pub fn highest(values: &[Decimal], period: usize) -> Option<Decimal> {
    if values.len() < period {
        return None;
    }
    values.iter().rev().take(period).max().copied()
}

pub fn lowest(values: &[Decimal], period: usize) -> Option<Decimal> {
    if values.len() < period {
        return None;
    }
    values.iter().rev().take(period).min().copied()
}

pub fn stddev(values: &[Decimal], period: usize) -> Option<Decimal> {
    if values.len() < period {
        return None;
    }
    let mean = sma(values, period)?;
    let variance: Decimal = values
        .iter()
        .rev()
        .take(period)
        .map(|v| {
            let diff = *v - mean;
            diff * diff
        })
        .sum::<Decimal>()
        / Decimal::from(period as u32);

    Some(sqrt_decimal(variance))
}

fn sqrt_decimal(value: Decimal) -> Decimal {
    if value.is_zero() || value.is_sign_negative() {
        return Decimal::ZERO;
    }

    let mut guess = value / Decimal::from(2);
    let epsilon = Decimal::new(1, 10); // 0.0000000001

    for _ in 0..50 {
        let new_guess = (guess + value / guess) / Decimal::from(2);
        if (new_guess - guess).abs() < epsilon {
            return new_guess;
        }
        guess = new_guess;
    }
    guess
}
