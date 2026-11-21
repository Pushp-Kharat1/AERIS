use aeris_core::{
    BookLevel, BookUpdate, Fixed8, OrderIntent, OrderType, Side, Symbol, TimeInForce, Venue,
};
use aeris_execution::{ExecutionRouter, FrictionModel};
use aeris_feed::{parse_binance_depth, parse_bybit_depth, DeadManSnitch, L2OrderBook};
use aeris_risk::{
    AccountRiskState, MetaLabelGate, MetaLabelGateConfig, OrphanedLegTracker, PreTradeRiskGateway,
    RiskGatewayConfig, RiskRejection,
};

// ==============================================================================
// LAYER 1: DATA POISONING & NUMERICAL CORRUPTION
// ==============================================================================

#[test]
fn test_l1_01_nan_and_inf_fixed8_sanitization() {
    let nan_fixed = Fixed8::from_f64(f64::NAN);
    assert_eq!(nan_fixed, Fixed8::ZERO);

    let inf_fixed = Fixed8::from_f64(f64::INFINITY);
    assert_eq!(inf_fixed, Fixed8::ZERO);

    let neg_inf_fixed = Fixed8::from_f64(f64::NEG_INFINITY);
    assert_eq!(neg_inf_fixed, Fixed8::ZERO);
}

#[test]
fn test_l1_02_zero_and_negative_order_quantity() {
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    let account = AccountRiskState {
        total_equity: Fixed8::from_f64(100_000.0),
        total_deployed_notional: Fixed8::ZERO,
    };

    let zero_qty_intent = OrderIntent {
        client_order_id: 1,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::ZERO,
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let res = gateway.evaluate_order(&zero_qty_intent, &account, Some(Fixed8::from_f64(60_000.0)));
    assert_eq!(
        res,
        Err(RiskRejection::InvalidOrderDimensions {
            price: Fixed8::from_f64(60_000.0),
            qty: Fixed8::ZERO,
        })
    );
}

#[test]
fn test_l1_03_astronomical_spread_price_deviation() {
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    let account = AccountRiskState {
        total_equity: Fixed8::from_f64(100_000.0),
        total_deployed_notional: Fixed8::ZERO,
    };

    let bbo = Fixed8::from_f64(60_000.0);
    // Order price deviates by ~1000 bps (66,000 vs 60,000 is 10% = 1000 bps)
    let bad_intent = OrderIntent {
        client_order_id: 2,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(66_000.0),
        qty: Fixed8::from_f64(0.1),
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let res = gateway.evaluate_order(&bad_intent, &account, Some(bbo));
    assert!(matches!(res, Err(RiskRejection::PriceDeviationExceeded { .. })));
}

#[test]
fn test_l1_04_fat_finger_limit_exceeded() {
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    let account = AccountRiskState {
        total_equity: Fixed8::from_f64(500_000.0),
        total_deployed_notional: Fixed8::ZERO,
    };

    // 2 BTC @ $60,000 = $120,000 (exceeds $100,000 max single order notional)
    let fat_finger = OrderIntent {
        client_order_id: 3,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(2.0),
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let res = gateway.evaluate_order(&fat_finger, &account, Some(Fixed8::from_f64(60_000.0)));
    assert_eq!(
        res,
        Err(RiskRejection::FatFingerLimitExceeded {
            notional: Fixed8::from_f64(120_000.0),
            max_limit: Fixed8::from_f64(100_000.0),
        })
    );
}

#[test]
fn test_l1_05_corrupted_binance_websocket_json() {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let corrupt_payloads = vec![
        "",
        "{",
        "{\"E\": 12345, \"bids\": [[\"nan\", \"1.0\"]]}",
        "null",
        "{\"unexpected\": true}",
    ];

    for payload in corrupt_payloads {
        let res = parse_binance_depth(payload, sym);
        if let Ok(update) = res {
            assert!(update.bids.is_empty() || update.bids[0].price == Fixed8::ZERO);
        }
    }
}

#[test]
fn test_l1_06_corrupted_bybit_websocket_json() {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let corrupt_payloads = vec![
        "",
        "{",
        "{\"type\": \"delta\"}",
        "null",
        "{\"data\": null}",
    ];

    for payload in corrupt_payloads {
        let res = parse_bybit_depth(payload, sym);
        assert!(res.is_err() || res.unwrap().is_none());
    }
}

// ==============================================================================
// LAYER 2: NETWORK, API & HEARTBEAT DROPOUTS
// ==============================================================================

#[test]
fn test_l2_01_deadman_snitch_stalled_venue() {
    let mut deadman = DeadManSnitch::new(3000);
    deadman.record_heartbeat(Venue::Binance, 1_000_000_000);

    // Current time is 4.5 seconds later (3.5s elapsed > 3.0s timeout)
    assert!(deadman.is_venue_stalled(Venue::Binance, 4_500_000_000));
}

#[test]
fn test_l2_02_deadman_snitch_active_venue() {
    let mut deadman = DeadManSnitch::new(3000);
    deadman.record_heartbeat(Venue::Bybit, 2_000_000_000);

    // Current time is 1.0 second later (< 3.0s timeout)
    assert!(!deadman.is_venue_stalled(Venue::Bybit, 3_000_000_000));
}

#[test]
fn test_l2_03_deadman_asymmetric_venue_dropout() {
    let mut deadman = DeadManSnitch::new(3000);
    deadman.record_heartbeat(Venue::Binance, 1_000_000_000);
    deadman.record_heartbeat(Venue::Bybit, 4_000_000_000);

    let check_time = 4_500_000_000;
    assert!(deadman.is_venue_stalled(Venue::Binance, check_time));
    assert!(!deadman.is_venue_stalled(Venue::Bybit, check_time));
}

#[test]
fn test_l2_04_empty_orderbook_resilience() {
    let book = L2OrderBook::new(Venue::Binance, Symbol::from_str_truncate("BTCUSDT"));
    assert!(book.best_bid().is_none());
    assert!(book.best_ask().is_none());
    assert!(book.mid_price().is_none());
    assert!(book.spread_bps().is_none());
    assert!(book.micro_price().is_none());
    assert!(book.micro_price_divergence_bps().is_none());
}

// ==============================================================================
// LAYER 3: CAPITAL EVAPORATION & INSOLVENT ACCOUNT
// ==============================================================================

#[test]
fn test_l3_01_negative_equity_blowout() {
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    let account = AccountRiskState {
        total_equity: Fixed8::from_f64(-15_000.0),
        total_deployed_notional: Fixed8::ZERO,
    };

    let intent = OrderIntent {
        client_order_id: 10,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(0.1),
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let res = gateway.evaluate_order(&intent, &account, Some(Fixed8::from_f64(60_000.0)));
    assert_eq!(
        res,
        Err(RiskRejection::InsolventAccount {
            equity: Fixed8::from_f64(-15_000.0),
        })
    );
}

#[test]
fn test_l3_02_zero_equity_rejection() {
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    let account = AccountRiskState {
        total_equity: Fixed8::ZERO,
        total_deployed_notional: Fixed8::ZERO,
    };

    let intent = OrderIntent {
        client_order_id: 11,
        venue: Venue::Bybit,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(0.1),
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let res = gateway.evaluate_order(&intent, &account, Some(Fixed8::from_f64(60_000.0)));
    assert_eq!(
        res,
        Err(RiskRejection::InsolventAccount {
            equity: Fixed8::ZERO,
        })
    );
}

#[test]
fn test_l3_03_hard_leverage_cap_breach() {
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    let account = AccountRiskState {
        total_equity: Fixed8::from_f64(100_000.0),
        total_deployed_notional: Fixed8::from_f64(240_000.0), // already 2.4x
    };

    // Adding $20,000 pushes deployed to $260,000 = 2.6x (> 2.5x max leverage)
    let intent = OrderIntent {
        client_order_id: 12,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(20_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let res = gateway.evaluate_order(&intent, &account, Some(Fixed8::from_f64(20_000.0)));
    assert_eq!(
        res,
        Err(RiskRejection::LeverageCapExceeded {
            projected_leverage: Fixed8::from_f64(2.6),
            max_leverage: Fixed8::from_f64(2.5),
        })
    );
}

#[test]
fn test_l3_04_cash_equity_desynchronization_trap() {
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    // Equity is only $10,000 even if external balance reports high cash
    let account = AccountRiskState {
        total_equity: Fixed8::from_f64(10_000.0),
        total_deployed_notional: Fixed8::ZERO,
    };

    // Attempting a $30,000 order = 3.0x leverage (rejected)
    let intent = OrderIntent {
        client_order_id: 13,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("ETHUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(3_000.0),
        qty: Fixed8::from_f64(10.0),
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let res = gateway.evaluate_order(&intent, &account, Some(Fixed8::from_f64(3_000.0)));
    assert_eq!(
        res,
        Err(RiskRejection::LeverageCapExceeded {
            projected_leverage: Fixed8::from_f64(3.0),
            max_leverage: Fixed8::from_f64(2.5),
        })
    );
}

// ==============================================================================
// LAYER 4: ML GATEKEEPER & REGIME BOUNDARY
// ==============================================================================

#[test]
fn test_l4_01_meta_label_hurdle_failure() {
    let gate = MetaLabelGate::new(MetaLabelGateConfig::default());
    let low_prob = Fixed8::from_f64(0.55); // < 0.60 hurdle
    let intent = OrderIntent {
        client_order_id: 1,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 1000,
        parent_leg_id: None,
    };
    let res = gate.evaluate_and_size(&intent, low_prob);
    assert_eq!(
        res,
        Err(RiskRejection::MetaLabelHurdleFailed {
            probability: low_prob,
            min_hurdle: Fixed8::from_f64(0.60),
        })
    );
}

#[test]
fn test_l4_02_meta_label_hurdle_pass_and_kelly_scaling() {
    let gate = MetaLabelGate::new(MetaLabelGateConfig::default());
    let high_prob = Fixed8::from_f64(0.80); // >= 0.60 hurdle
    let intent = OrderIntent {
        client_order_id: 2,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 1000,
        parent_leg_id: None,
    };
    let sized = gate.evaluate_and_size(&intent, high_prob).unwrap();

    // Fractional Kelly: 2 * 0.80 - 1 = 0.60
    assert_eq!(sized.qty, Fixed8::from_f64(0.60));
}

#[test]
fn test_l4_03_meta_label_kelly_ceiling_clamp() {
    let gate = MetaLabelGate::new(MetaLabelGateConfig::default());
    let perfect_prob = Fixed8::from_f64(1.0);
    let intent = OrderIntent {
        client_order_id: 3,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 1000,
        parent_leg_id: None,
    };
    let sized = gate.evaluate_and_size(&intent, perfect_prob).unwrap();

    // Max kelly fraction default is 1.00
    assert_eq!(sized.qty, Fixed8::from_f64(1.0));
}

#[test]
fn test_l4_04_stoikov_micro_price_zero_depth_safety() {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let mut book = L2OrderBook::new(Venue::Binance, sym);

    // 1. Zero quantity updates correctly delete/ignore levels (L2 book semantics)
    let zero_update = BookUpdate {
        venue: Venue::Binance,
        symbol: sym,
        timestamp_ns: 1000,
        bids: vec![BookLevel {
            price: Fixed8::from_f64(60_000.0),
            qty: Fixed8::ZERO,
        }],
        asks: vec![BookLevel {
            price: Fixed8::from_f64(60_010.0),
            qty: Fixed8::ZERO,
        }],
        is_snapshot: true,
    };
    book.apply_update(&zero_update);
    // When depth is empty, micro_price safely returns None without panicking
    assert!(book.micro_price().is_none());

    // 2. Balanced depth returns exact midpoint
    let balanced_update = BookUpdate {
        venue: Venue::Binance,
        symbol: sym,
        timestamp_ns: 2000,
        bids: vec![BookLevel {
            price: Fixed8::from_f64(60_000.0),
            qty: Fixed8::from_f64(2.0),
        }],
        asks: vec![BookLevel {
            price: Fixed8::from_f64(60_010.0),
            qty: Fixed8::from_f64(2.0),
        }],
        is_snapshot: true,
    };
    book.apply_update(&balanced_update);
    assert_eq!(book.micro_price().unwrap(), Fixed8::from_f64(60_005.0));

    // 3. Heavy bid depth (10 BTC bids vs 2 BTC asks) tilts micro-price toward ask
    let skewed_update = BookUpdate {
        venue: Venue::Binance,
        symbol: sym,
        timestamp_ns: 3000,
        bids: vec![BookLevel {
            price: Fixed8::from_f64(60_000.0),
            qty: Fixed8::from_f64(10.0),
        }],
        asks: vec![BookLevel {
            price: Fixed8::from_f64(60_010.0),
            qty: Fixed8::from_f64(2.0),
        }],
        is_snapshot: true,
    };
    book.apply_update(&skewed_update);
    let skewed_micro = book.micro_price().unwrap();
    // Micro-price with heavy bid pressure must be strictly greater than mid ($60,005)
    assert!(skewed_micro > Fixed8::from_f64(60_005.0));
}

// ==============================================================================
// LAYER 5: CONCURRENCY, TIME SKEW & EXECUTION SAFETY
// ==============================================================================

#[test]
fn test_l5_01_orphaned_leg_timeout_emergency_ioc() {
    let mut tracker = OrphanedLegTracker::new(50); // 50ms timeout

    let leg_a = OrderIntent {
        client_order_id: 101,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 0,
        parent_leg_id: None,
    };

    let leg_b = OrderIntent {
        client_order_id: 102,
        venue: Venue::Bybit,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_005.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 0,
        parent_leg_id: None,
    };

    tracker.register_spread(1, leg_a, leg_b);

    // Leg A fills at t=10ms
    tracker.record_fill(101, 10_000_000);

    // At t=50ms (40ms elapsed), no timeout yet
    let unwinds_early = tracker.check_timeouts(50_000_000);
    assert!(unwinds_early.is_empty());

    // At t=65ms (55ms elapsed > 50ms), emergency IOC market unwind generated
    let unwinds_late = tracker.check_timeouts(65_000_000);
    assert_eq!(unwinds_late.len(), 1);
    let unwind = &unwinds_late[0];
    assert_eq!(unwind.side, Side::Sell); // Opposite of Leg A (Buy -> Sell)
    assert_eq!(unwind.order_type, OrderType::ImmediateOrCancel);
    assert_eq!(unwind.parent_leg_id, Some(101));
}

#[test]
fn test_l5_02_orphaned_leg_cleared_on_leg_b_fill() {
    let mut tracker = OrphanedLegTracker::new(50);

    let leg_a = OrderIntent {
        client_order_id: 201,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 0,
        parent_leg_id: None,
    };

    let leg_b = OrderIntent {
        client_order_id: 202,
        venue: Venue::Bybit,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_005.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 0,
        parent_leg_id: None,
    };

    tracker.register_spread(2, leg_a, leg_b);

    // Leg A fills at t=10ms, Leg B fills at t=35ms (< 50ms)
    tracker.record_fill(201, 10_000_000);
    tracker.record_fill(202, 35_000_000);

    // At t=100ms, no unwinds because both legs completed
    let unwinds = tracker.check_timeouts(100_000_000);
    assert!(unwinds.is_empty());
}

#[test]
fn test_l5_03_ntp_backward_clock_skew() {
    let mut tracker = OrphanedLegTracker::new(50);

    let leg_a = OrderIntent {
        client_order_id: 301,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 100_000_000,
        parent_leg_id: None,
    };
    let leg_b = OrderIntent {
        client_order_id: 302,
        venue: Venue::Bybit,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_005.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 100_000_000,
        parent_leg_id: None,
    };

    tracker.register_spread(3, leg_a, leg_b);
    tracker.record_fill(301, 100_000_000);

    // Backward time jump: current time is 50_000_000 (< fill time 100_000_000)
    // saturating_sub should prevent underflow and zero unwinds generated
    let unwinds = tracker.check_timeouts(50_000_000);
    assert!(unwinds.is_empty());
}

#[test]
fn test_l5_04_ntp_forward_clock_jump() {
    let mut tracker = OrphanedLegTracker::new(50);

    let leg_a = OrderIntent {
        client_order_id: 401,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 10_000_000,
        parent_leg_id: None,
    };
    let leg_b = OrderIntent {
        client_order_id: 402,
        venue: Venue::Bybit,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_005.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: 10_000_000,
        parent_leg_id: None,
    };

    tracker.register_spread(4, leg_a, leg_b);
    tracker.record_fill(401, 10_000_000);

    // Forward time jump of 1 hour (3600 seconds = 3,600,000,000,000 ns)
    let unwinds = tracker.check_timeouts(10_000_000 + 3_600_000_000_000);
    assert_eq!(unwinds.len(), 1);
    assert_eq!(unwinds[0].parent_leg_id, Some(401));
}

#[test]
fn test_l5_05_friction_and_indian_194s_tds() {
    let friction = FrictionModel::default();
    let notional = Fixed8::from_f64(100_000.0);

    // 1% TDS on Sell = $1,000.00
    let sell_tds = friction.compute_tds(notional, Side::Sell);
    assert_eq!(sell_tds, Fixed8::from_f64(1_000.0));

    // 0% TDS on Buy
    let buy_tds = friction.compute_tds(notional, Side::Buy);
    assert_eq!(buy_tds, Fixed8::ZERO);

    // Execution Router rejects insolvent order and blocks fill
    let gateway = PreTradeRiskGateway::new(RiskGatewayConfig::default());
    let router = ExecutionRouter::new(gateway, friction);
    let insolvent_account = AccountRiskState {
        total_equity: Fixed8::from_f64(-1.0),
        total_deployed_notional: Fixed8::ZERO,
    };

    let intent = OrderIntent {
        client_order_id: 501,
        venue: Venue::Binance,
        symbol: Symbol::from_str_truncate("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: Fixed8::from_f64(60_000.0),
        qty: Fixed8::from_f64(0.1),
        created_at_ns: 1000,
        parent_leg_id: None,
    };

    let report = router.validate_and_route(&intent, &insolvent_account, Some(Fixed8::from_f64(60_000.0)));
    assert!(report.is_err()); // Non-bypassable: completely rejected before routing
}
