pub mod breakout;
pub mod counter_punch;
pub mod cross_venue;
pub mod micro_momentum;

pub use breakout::*;
pub use counter_punch::*;
pub use cross_venue::*;
pub use micro_momentum::*;

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_core::{BookLevel, BookUpdate, Fixed8, Symbol, Venue};
    use aeris_feed::L2OrderBook;

    #[test]
    fn test_cross_venue_spread_detection_and_net_profit() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut binance_book = L2OrderBook::new(Venue::Binance, sym);
        let mut bybit_book = L2OrderBook::new(Venue::Bybit, sym);

        // Bybit Ask is $60,000, Binance Bid is $60,800 (+800 USD = +133 bps)
        // Fees: 10 bps taker roundtrip + 100 bps TDS = 110 bps
        // Net spread: 133.3 bps - 110 bps = +23.3 bps (Profitable!)
        bybit_book.apply_update(&BookUpdate {
            venue: Venue::Bybit,
            symbol: sym,
            timestamp_ns: 1000,
            bids: vec![BookLevel { price: Fixed8::from_f64(59990.0), qty: Fixed8::from_f64(1.0) }],
            asks: vec![BookLevel { price: Fixed8::from_f64(60000.0), qty: Fixed8::from_f64(2.5) }],
            is_snapshot: true,
        });

        binance_book.apply_update(&BookUpdate {
            venue: Venue::Binance,
            symbol: sym,
            timestamp_ns: 1000,
            bids: vec![BookLevel { price: Fixed8::from_f64(60800.0), qty: Fixed8::from_f64(1.5) }],
            asks: vec![BookLevel { price: Fixed8::from_f64(60810.0), qty: Fixed8::from_f64(1.0) }],
            is_snapshot: true,
        });

        let mut engine = CrossVenueArbEngine::new(
            Fixed8::from_f64(5.0), // 5 bps net hurdle
            Fixed8::from_f64(100_000.0),
        );

        let opp = engine.inspect_spread(sym, &binance_book, &bybit_book).unwrap();
        assert_eq!(opp.buy_venue, Venue::Bybit);
        assert_eq!(opp.sell_venue, Venue::Binance);
        assert_eq!(opp.max_qty, Fixed8::from_f64(1.5)); // min(2.5, 1.5)
        assert!(opp.gross_spread_bps.to_f64() > 130.0);
        assert!(opp.net_spread_bps.to_f64() > 20.0);

        let spread_intent = engine.generate_spread_intent(&opp, 2000).unwrap();
        assert_eq!(spread_intent.leg_a_buy.venue, Venue::Bybit);
        assert_eq!(spread_intent.leg_b_sell.venue, Venue::Binance);
        assert_eq!(spread_intent.executable_qty, Fixed8::from_f64(1.5));
    }

    #[test]
    fn test_cross_venue_unprofitable_spread_filtered() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut binance_book = L2OrderBook::new(Venue::Binance, sym);
        let mut bybit_book = L2OrderBook::new(Venue::Bybit, sym);

        // Only 20 bps gross spread (not enough to cover 110 bps friction)
        bybit_book.apply_update(&BookUpdate {
            venue: Venue::Bybit,
            symbol: sym,
            timestamp_ns: 1000,
            bids: vec![BookLevel { price: Fixed8::from_f64(59990.0), qty: Fixed8::from_f64(1.0) }],
            asks: vec![BookLevel { price: Fixed8::from_f64(60000.0), qty: Fixed8::from_f64(1.0) }],
            is_snapshot: true,
        });

        binance_book.apply_update(&BookUpdate {
            venue: Venue::Binance,
            symbol: sym,
            timestamp_ns: 1000,
            bids: vec![BookLevel { price: Fixed8::from_f64(60020.0), qty: Fixed8::from_f64(1.0) }],
            asks: vec![BookLevel { price: Fixed8::from_f64(60030.0), qty: Fixed8::from_f64(1.0) }],
            is_snapshot: true,
        });

        let mut engine = CrossVenueArbEngine::new(
            Fixed8::from_f64(5.0),
            Fixed8::from_f64(100_000.0),
        );

        let opp = engine.inspect_spread(sym, &binance_book, &bybit_book).unwrap();
        // Net spread should be negative (-76.6 bps)
        assert!(opp.net_spread_bps < Fixed8::ZERO);

        let spread_intent = engine.generate_spread_intent(&opp, 2000);
        assert!(spread_intent.is_none());
    }

    #[test]
    fn test_micro_momentum_macro_conditioning() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut book = L2OrderBook::new(Venue::Binance, sym);

        // Heavy bid imbalance tilting micro-price upward
        book.apply_update(&BookUpdate {
            venue: Venue::Binance,
            symbol: sym,
            timestamp_ns: 1000,
            bids: vec![BookLevel { price: Fixed8::from_f64(60000.0), qty: Fixed8::from_f64(10.0) }],
            asks: vec![BookLevel { price: Fixed8::from_f64(60010.0), qty: Fixed8::from_f64(1.0) }],
            is_snapshot: true,
        });

        let mut engine = MicroMomentumEngine::new(
            Fixed8::from_f64(0.5), // 0.5 bps hurdle (spread is 1.66 bps)
            Fixed8::from_f64(1.0), // 1 BTC default
        );

        // 1. Confirmed by SUPPLY_SQUEEZE macro (bias = 1): Generates Buy order
        let intent_aligned = engine.evaluate_signal(&book, 1, 2000);
        assert!(intent_aligned.is_some());
        let order = intent_aligned.unwrap();
        assert_eq!(order.side, aeris_core::Side::Buy);
        // Kelly sizing: 2 * 0.75 - 1 = 0.50
        assert_eq!(order.qty, Fixed8::from_f64(0.50));

        // 2. Contradicted by DISTRIBUTION macro (bias = 2): Probability 0.45 < 0.60 hurdle -> Filtered!
        let intent_opposed = engine.evaluate_signal(&book, 2, 2000);
        assert!(intent_opposed.is_none());
    }
}
