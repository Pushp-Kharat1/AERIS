pub mod friction;
pub mod router;

pub use friction::*;
pub use router::*;

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_core::{Fixed8, OrderIntent, OrderType, Side, Symbol, TimeInForce, Venue};
    use aeris_risk::{AccountRiskState, PreTradeRiskGateway, RiskGatewayConfig};

    #[test]
    fn test_friction_and_tds_calculation() {
        let friction = FrictionModel::default();
        let notional = Fixed8::from_f64(10_000.0);

        // Taker fee: 5 bps = 10,000 * 0.0005 = $5.00
        let taker_fee = friction.compute_fee(notional, false);
        assert_eq!(taker_fee.to_f64(), 5.0);

        // TDS on sell: 1% = $100.00
        let tds = friction.compute_tds(notional, Side::Sell);
        assert_eq!(tds.to_f64(), 100.0);

        // TDS on buy: $0.00
        let buy_tds = friction.compute_tds(notional, Side::Buy);
        assert_eq!(buy_tds.to_f64(), 0.0);
    }

    #[test]
    fn test_router_execution_flow() {
        let risk = PreTradeRiskGateway::new(RiskGatewayConfig::default());
        let friction = FrictionModel::default();
        let router = ExecutionRouter::new(risk, friction);

        let account = AccountRiskState {
            total_equity: Fixed8::from_f64(50_000.0),
            total_deployed_notional: Fixed8::from_f64(20_000.0),
        };

        let intent = OrderIntent {
            client_order_id: 42,
            venue: Venue::Binance,
            symbol: Symbol::from_str_truncate("BTCUSDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: Fixed8::from_f64(60_000.0),
            qty: Fixed8::from_f64(0.1), // 6,000 notional
            created_at_ns: 1000,
            parent_leg_id: None,
        };

        let routed = router.validate_and_route(&intent, &account, Some(Fixed8::from_f64(60_000.0))).unwrap();
        assert_eq!(routed.client_order_id, 42);

        let report = router.process_fill(&routed, Fixed8::from_f64(60_000.0), Fixed8::from_f64(0.1), false, 1050);
        assert_eq!(report.fee_paid.to_f64(), 3.0); // 6,000 * 5 bps = $3.00
        assert_eq!(report.tds_paid.to_f64(), 0.0); // Buy has 0 TDS
    }
}
