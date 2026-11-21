pub mod binance_ws;
pub mod bybit_ws;
pub mod deadman;
pub mod orderbook;

pub use binance_ws::*;
pub use bybit_ws::*;
pub use deadman::*;
pub use orderbook::*;

pub fn init_crypto_provider() {
    rustls::crypto::ring::default_provider().install_default().ok();
}

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

    #[test]
    fn test_binance_depth_parser() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let sample = r#"{
            "E": 1795020786580,
            "bids": [["60000.50", "1.500"], ["59990.00", "5.000"]],
            "asks": [["60010.50", "2.000"]]
        }"#;

        let update = parse_binance_depth(sample, sym).unwrap();
        assert_eq!(update.venue, Venue::Binance);
        assert_eq!(update.bids.len(), 2);
        assert_eq!(update.bids[0].price.to_f64(), 60000.50);
        assert_eq!(update.asks[0].price.to_f64(), 60010.50);
    }

    #[test]
    fn test_bybit_depth_parser() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let sample = r#"{
            "topic": "orderbook.50.BTCUSDT",
            "type": "snapshot",
            "ts": 1795020786580,
            "data": {
                "b": [["60002.00", "0.800"]],
                "a": [["60005.00", "1.200"]]
            }
        }"#;

        let update = parse_bybit_depth(sample, sym).unwrap().unwrap();
        assert_eq!(update.venue, Venue::Bybit);
        assert!(update.is_snapshot);
        assert_eq!(update.bids[0].price.to_f64(), 60002.00);
    }
}
