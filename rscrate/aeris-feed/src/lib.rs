pub mod deadman;
pub mod orderbook;

pub use deadman::*;
pub use orderbook::*;

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_core::{BookLevel, BookUpdate, Fixed8, Symbol, Venue};

    #[test]
    fn test_orderbook_bbo_and_spread() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut book = L2OrderBook::new(Venue::Binance, sym);

        let update = BookUpdate {
            venue: Venue::Binance,
            symbol: sym,
            timestamp_ns: 1000,
            bids: vec![
                BookLevel { price: Fixed8::from_f64(60000.0), qty: Fixed8::from_f64(1.5) },
                BookLevel { price: Fixed8::from_f64(59990.0), qty: Fixed8::from_f64(3.0) },
            ],
            asks: vec![
                BookLevel { price: Fixed8::from_f64(60010.0), qty: Fixed8::from_f64(2.0) },
                BookLevel { price: Fixed8::from_f64(60020.0), qty: Fixed8::from_f64(4.0) },
            ],
            is_snapshot: true,
        };

        book.apply_update(&update);

        let bbo_bid = book.best_bid().unwrap();
        let bbo_ask = book.best_ask().unwrap();

        assert_eq!(bbo_bid.price.to_f64(), 60000.0);
        assert_eq!(bbo_ask.price.to_f64(), 60010.0);

        let spread = book.spread_bps().unwrap();
        // 10 / 60000 * 10000 = 1.6666 bps
        assert!((spread.to_f64() - 1.66666).abs() < 0.01);
    }

    #[test]
    fn test_micro_price_calculation() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut book = L2OrderBook::new(Venue::Binance, sym);

        let update = BookUpdate {
            venue: Venue::Binance,
            symbol: sym,
            timestamp_ns: 1000,
            bids: vec![
                BookLevel { price: Fixed8::from_f64(60000.0), qty: Fixed8::from_f64(9.0) }, // Heavy bid
            ],
            asks: vec![
                BookLevel { price: Fixed8::from_f64(60010.0), qty: Fixed8::from_f64(1.0) }, // Thin ask
            ],
            is_snapshot: true,
        };

        book.apply_update(&update);

        let mid = book.mid_price().unwrap();
        assert_eq!(mid.to_f64(), 60005.0);

        let micro = book.micro_price().unwrap();
        // (9 * 60010 + 1 * 60000) / 10 = 60009.0
        assert_eq!(micro.to_f64(), 60009.0);

        let divergence = book.micro_price_divergence_bps().unwrap();
        assert!(divergence.to_f64() > 0.0);
    }
}
