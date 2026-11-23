pub mod config;
pub mod engine;
pub mod events;
pub mod generator;
pub mod queue;

pub use config::BacktestConfig;
pub use engine::{BacktestReport, ClosedTradeRecord, FeeAndFillSimulator};
pub use events::{LiquidationPrint, MarketEvent, TradePrint};
pub use generator::SyntheticMarketGenerator;
pub use queue::SimulatedOrderQueue;

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_core::{Fixed8, OrderIntent, OrderType, Side, Symbol, TimeInForce, Venue};

    #[test]
    fn test_latency_buffer_delays_taker_execution() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));
        let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, BacktestConfig::remote_mumbai()); // 95ms latency

        // Ingest initial book
        let l2 = gen.generate_l2_update(1.0, 1.0);
        sim.process_event(l2, gen.current_time_ns);

        // Submit IOC Buy order at t = 0ms
        let t0 = gen.current_time_ns;
        let intent = OrderIntent {
            client_order_id: 101,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: Fixed8::from_f64(90_050.0),
            qty: Fixed8::from_f64(0.1),
            created_at_ns: t0,
            parent_leg_id: None,
        };
        sim.submit_order(intent, t0);

        // Advance 50ms (order should still be in-flight)
        let t50 = gen.advance_time(50);
        let l2_mid = gen.generate_l2_update(1.0, 1.0);
        sim.process_event(l2_mid, t50);

        assert_eq!(sim.positions.len(), 0, "Order must NOT execute before 95ms network latency!");

        // Advance past 95ms (to 100ms) - order should now execute
        let t100 = gen.advance_time(50);
        let l2_100 = gen.generate_l2_update(1.0, 1.0);
        sim.process_event(l2_100, t100);

        assert_eq!(sim.positions.len(), 1, "Order must execute once 95ms latency buffer elapses!");
    }

    #[test]
    fn test_maker_queue_exhaustion_required_for_fill() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));
        let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, BacktestConfig::tokyo_colocated()); // 1ms latency

        // Book has 1.0 BTC depth at best bid (~$89,995.5)
        let l2 = gen.generate_l2_update(1.0, 1.0);
        sim.process_event(l2, gen.current_time_ns);

        let bid_px = sim.book.best_bid().unwrap().price;

        // Place resting maker bid of 0.2 BTC behind the existing 1.0 BTC queue
        let t0 = gen.current_time_ns;
        let intent = OrderIntent {
            client_order_id: 202,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: bid_px,
            qty: Fixed8::from_f64(0.2),
            created_at_ns: t0,
            parent_leg_id: None,
        };
        sim.submit_order(intent, t0);

        // Advance past 1ms to activate order in queue
        let t2 = gen.advance_time(2);
        let trade_small = MarketEvent::Trade(TradePrint {
            price: bid_px,
            qty: Fixed8::from_f64(0.5), // Only 0.5 BTC sold, queue had 1.0 BTC ahead
            is_buyer_maker: true,
            timestamp_ns: t2,
        });
        sim.process_event(trade_small, t2);

        assert_eq!(sim.positions.len(), 0, "Maker order must NOT fill if queue ahead is not exhausted!");

        // Next trade sells 0.7 BTC (exhausts remaining 0.5 BTC and fills our 0.2 BTC)
        let t3 = gen.advance_time(2);
        let trade_large = MarketEvent::Trade(TradePrint {
            price: bid_px,
            qty: Fixed8::from_f64(0.7),
            is_buyer_maker: true,
            timestamp_ns: t3,
        });
        sim.process_event(trade_large, t3);

        assert_eq!(sim.positions.len(), 1, "Maker order must fill once queue ahead is consumed!");
    }

    #[test]
    fn test_vip0_taker_fee_burn_revealed_on_micro_gain() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));
        let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, BacktestConfig::tokyo_colocated());

        // Initial book
        let l2 = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2, gen.current_time_ns);

        // Enter Buy IOC at $90,004.5 (paying 5 bps taker fee)
        let t1 = gen.advance_time(2);
        let buy_intent = OrderIntent {
            client_order_id: 301,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: Fixed8::from_f64(90_050.0),
            qty: Fixed8::from_f64(1.0),
            created_at_ns: t1,
            parent_leg_id: None,
        };
        sim.submit_order(buy_intent, t1);
        let l2_fill = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_fill, gen.advance_time(2));

        // Market drifts up by +1.5 bps (tiny microscopic gain)
        gen.current_mid = gen.current_mid + Fixed8::from_f64(13.5); // +1.5 bps on $90k
        let t2 = gen.advance_time(10);
        let l2_up = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_up, t2);

        // Exit Sell IOC (paying another 5 bps taker fee)
        let sell_intent = OrderIntent {
            client_order_id: 302,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Sell,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: Fixed8::from_f64(80_000.0),
            qty: Fixed8::from_f64(1.0),
            created_at_ns: t2,
            parent_leg_id: None,
        };
        sim.submit_order(sell_intent, t2);
        let l2_exit = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_exit, gen.advance_time(2));

        let report = sim.generate_report();
        assert_eq!(report.total_trades, 1);
        assert!(report.gross_realized_pnl.is_positive(), "Gross PnL was positive (+1.5 bps drift)");
        assert!(report.net_realized_pnl.is_negative(), "Net PnL MUST be deeply negative after paying 10 bps round-trip fees!");
        assert_eq!(report.gross_winning_trades, 1);
        assert_eq!(report.net_winning_trades, 0, "Net win rate must be 0% due to taker fee drag!");
    }

    #[test]
    fn test_adverse_selection_penalty_on_maker_fill() {
        let sym = Symbol::from_str_truncate("BTCUSDT");
        let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));
        let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, BacktestConfig::tokyo_colocated());

        let l2 = gen.generate_l2_update(1.0, 1.0);
        sim.process_event(l2, gen.current_time_ns);

        let bid_px = sim.book.best_bid().unwrap().price;

        let t0 = gen.current_time_ns;
        let intent = OrderIntent {
            client_order_id: 401,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: bid_px,
            qty: Fixed8::from_f64(0.5),
            created_at_ns: t0,
            parent_leg_id: None,
        };
        sim.submit_order(intent, t0);

        // Fill with aggressive sweep
        let t1 = gen.advance_time(5);
        let trade = MarketEvent::Trade(TradePrint {
            price: bid_px,
            qty: Fixed8::from_f64(2.0),
            is_buyer_maker: true,
            timestamp_ns: t1,
        });
        sim.process_event(trade, t1);

        let pos = sim.positions.get(&sym).unwrap();
        // With 2.5 bps adverse selection, effective entry price is penalised upwards (higher cost) from limit price
        assert!(pos.avg_entry_price > bid_px, "Adverse selection penalty must shift effective entry price!");
    }
}
