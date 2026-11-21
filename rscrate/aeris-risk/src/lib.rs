pub mod gateway;
pub mod orphaned_leg;

pub use gateway::*;
pub use orphaned_leg::*;

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_core::{Fixed8, OrderIntent, OrderType, Side, Symbol, TimeInForce, Venue};

    #[test]
    fn test_risk_gateway_leverage_limit() {
        let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
        let account = AccountRiskState {
            total_equity: Fixed8::from_f64(100_000.0),
            total_deployed_notional: Fixed8::from_f64(240_000.0),
        };

        let intent = OrderIntent {
            client_order_id: 1,
            venue: Venue::Binance,
            symbol: Symbol::from_str_truncate("BTCUSDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: Fixed8::from_f64(60_000.0),
            qty: Fixed8::from_f64(0.5), // notional = 30,000 -> 240k + 30k = 270k / 100k = 2.7x > 2.5x
            created_at_ns: 1000,
            parent_leg_id: None,
        };

        let res = gateway.evaluate_order(&intent, &account, Some(Fixed8::from_f64(60_000.0)));
        assert!(matches!(res, Err(RiskRejection::LeverageCapExceeded { .. })));
    }

    #[test]
    fn test_orphaned_leg_timeout() {
        let mut tracker = OrphanedLegTracker::new(50); // 50ms timeout

        let leg_a = OrderIntent {
            client_order_id: 101,
            venue: Venue::Binance,
            symbol: Symbol::from_str_truncate("AVAXUSDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: Fixed8::from_f64(30.0),
            qty: Fixed8::from_f64(100.0),
            created_at_ns: 1_000_000_000,
            parent_leg_id: None,
        };

        let leg_b = OrderIntent {
            client_order_id: 102,
            venue: Venue::Bybit,
            symbol: Symbol::from_str_truncate("AVAXUSDT"),
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: Fixed8::from_f64(30.1),
            qty: Fixed8::from_f64(100.0),
            created_at_ns: 1_000_000_000,
            parent_leg_id: None,
        };

        tracker.register_spread(1, leg_a, leg_b);
        // Leg A fills at t = 1.000s
        tracker.record_fill(101, 1_000_000_000);

        // At t = 1.020s (20ms elapsed), no timeout yet
        let unwinds = tracker.check_timeouts(1_020_000_000);
        assert!(unwinds.is_empty());

        // At t = 1.060s (60ms elapsed > 50ms), Leg B never filled -> trigger emergency unwind of Leg A!
        let unwinds = tracker.check_timeouts(1_060_000_000);
        assert_eq!(unwinds.len(), 1);
        let emergency = &unwinds[0];
        assert_eq!(emergency.side, Side::Sell);
        assert_eq!(emergency.order_type, OrderType::ImmediateOrCancel);
        assert_eq!(emergency.qty, Fixed8::from_f64(100.0));
    }
}
