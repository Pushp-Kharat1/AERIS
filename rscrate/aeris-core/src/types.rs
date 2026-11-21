use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

pub const SCALE: i64 = 100_000_000;

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Fixed8(i64);

impl Fixed8 {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(SCALE);
    pub const MAX: Self = Self(i64::MAX);
    pub const MIN: Self = Self(i64::MIN);

    #[inline(always)]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub const fn raw(self) -> i64 {
        self.0
    }

    #[inline(always)]
    pub const fn from_int(v: i64) -> Self {
        Self(v * SCALE)
    }

    #[inline(always)]
    pub fn from_f64(v: f64) -> Self {
        Self((v * SCALE as f64).round() as i64)
    }

    #[inline(always)]
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / SCALE as f64
    }

    #[inline(always)]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[inline(always)]
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    #[inline(always)]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    #[inline(always)]
    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }
}

impl fmt::Debug for Fixed8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fixed8({:.8})", self.to_f64())
    }
}

impl fmt::Display for Fixed8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.8}", self.to_f64())
    }
}

impl Add for Fixed8 {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Fixed8 {
    #[inline(always)]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Fixed8 {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Fixed8 {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul for Fixed8 {
    type Output = Self;
    #[inline(always)]
    fn mul(self, rhs: Self) -> Self {
        let prod = (self.0 as i128) * (rhs.0 as i128);
        Self((prod / SCALE as i128) as i64)
    }
}

impl MulAssign for Fixed8 {
    #[inline(always)]
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl Div for Fixed8 {
    type Output = Self;
    #[inline(always)]
    fn div(self, rhs: Self) -> Self {
        if rhs.0 == 0 {
            panic!("Fixed8 divide by zero");
        }
        let numer = (self.0 as i128) * (SCALE as i128);
        Self((numer / rhs.0 as i128) as i64)
    }
}

impl DivAssign for Fixed8 {
    #[inline(always)]
    fn div_assign(&mut self, rhs: Self) {
        *self = *self / rhs;
    }
}

impl Neg for Fixed8 {
    type Output = Self;
    #[inline(always)]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Symbol([u8; 16]);

impl Symbol {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn from_str_truncate(s: &str) -> Self {
        let mut buf = [0u8; 16];
        let bytes = s.as_bytes();
        let len = bytes.len().min(16);
        buf[..len].copy_from_slice(&bytes[..len]);
        Self(buf)
    }

    pub fn as_str(&self) -> &str {
        let len = self.0.iter().position(|&b| b == 0).unwrap_or(16);
        std::str::from_utf8(&self.0[..len]).unwrap_or("")
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbol({})", self.as_str())
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub type TimestampNs = u64;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    #[inline(always)]
    pub fn opposite(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Venue {
    Binance,
    Bybit,
    Hyperliquid,
    Simulated,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
    ImmediateOrCancel,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeInForce {
    GoodTilCancelled,
    ImmediateOrCancel,
    FillOrKill,
    PostOnly,
}
