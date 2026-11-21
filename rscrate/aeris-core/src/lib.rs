pub mod events;
pub mod types;

pub use events::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed8_arithmetic() {
        let a = Fixed8::from_f64(10.5);
        let b = Fixed8::from_f64(2.0);

        let sum = a + b;
        assert_eq!(sum.to_f64(), 12.5);

        let diff = a - b;
        assert_eq!(diff.to_f64(), 8.5);

        let prod = a * b;
        assert_eq!(prod.to_f64(), 21.0);

        let quot = a / b;
        assert_eq!(quot.to_f64(), 5.25);
    }

    #[test]
    fn test_symbol_roundtrip() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        assert_eq!(sym.as_str(), "BTCUSDT");
    }
}
