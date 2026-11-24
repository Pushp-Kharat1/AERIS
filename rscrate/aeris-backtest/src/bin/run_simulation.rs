use aeris_core::{Fixed8, OrderIntent, OrderType, Side, Symbol, TimeInForce, Venue};
use aeris_backtest::{BacktestConfig, BacktestReport, FeeAndFillSimulator, MarketEvent, SyntheticMarketGenerator, TradePrint};

fn print_simulation_report(title: &str, report: &BacktestReport, config: &BacktestConfig) {
    println!("\n================================================================================");
    println!(" {:^78} ", title);
    println!("================================================================================");
    println!(" CONFIGURATION:");
    println!("   * Network Latency     : {} ms", config.network_latency_ms);
    println!("   * Taker Fee           : {:.1} bps ({:.3}%)", config.taker_fee_bps.to_f64(), config.taker_fee_bps.to_f64() / 100.0);
    println!("   * Maker Fee           : {:.1} bps ({:.3}%)", config.maker_fee_bps.to_f64(), config.maker_fee_bps.to_f64() / 100.0);
    println!("   * Adverse Selection   : {:.1} bps penalty", config.adverse_selection_bps.to_f64());
    println!("--------------------------------------------------------------------------------");
    println!(" PERFORMANCE SCORECARD:");
    println!("   * Total Closed Trades : {}", report.total_trades);
    println!("   * Gross Win Rate      : {:.1}% ({}/{} trades)", report.gross_win_rate_pct, report.gross_winning_trades, report.total_trades);
    println!("   * Net Win Rate        : {:.1}% ({}/{} trades)", report.net_win_rate_pct, report.net_winning_trades, report.total_trades);
    println!("   * Gross Realized PnL  : ${:+10.2}", report.gross_realized_pnl.to_f64());
    println!("   * Total Fees Paid     : -${:9.2}", report.total_fees_paid.to_f64());
    println!("   * Net Realized PnL    : ${:+10.2}", report.net_realized_pnl.to_f64());
    println!("   * Fee/Alpha Burn Ratio: {:.2}x", report.fee_to_alpha_burn_ratio);
    println!("   * Profit Factor       : {:.2}", report.profit_factor);
    println!("   * Max Drawdown        : ${:.2} ({:.2}%)", report.max_drawdown_usd.to_f64(), report.max_drawdown_pct);
    println!("   * Final Equity        : ${:.2}", report.final_equity.to_f64());
    println!("================================================================================\n");
}

fn run_uncalibrated_soak_sim() -> (BacktestReport, BacktestConfig) {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let config = BacktestConfig::remote_mumbai(); // 95ms latency, VIP 0 fees
    let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, config.clone());
    let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));

    // Simulate 300 cycles of hyperactive micro-trading on 0.005 bps noise
    for i in 0..150 {
        gen.advance_time(100);
        let l2 = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2, gen.current_time_ns);

        // Micro-momentum triggers Buy IOC on 0.5 bps micro-tick
        let buy_intent = OrderIntent {
            client_order_id: (i * 2 + 1) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: Fixed8::from_f64(90_050.0),
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(buy_intent, gen.current_time_ns);

        // Advance 100ms (latency transit)
        gen.advance_time(100);
        gen.current_mid = gen.current_mid + Fixed8::from_f64(4.5); // +0.5 bps positive price movement
        let l2_fill = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_fill, gen.current_time_ns);

        // Exit Sell IOC after 500ms
        gen.advance_time(500);
        let sell_intent = OrderIntent {
            client_order_id: (i * 2 + 2) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Sell,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: Fixed8::from_f64(80_000.0),
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(sell_intent, gen.current_time_ns);
        gen.advance_time(100);
        let l2_exit = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_exit, gen.current_time_ns);
    }

    (sim.generate_report(), config)
}

fn run_calibrated_maker_sim() -> (BacktestReport, BacktestConfig) {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let config = BacktestConfig::remote_mumbai(); // 95ms latency, VIP 0 fees
    let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, config.clone());
    let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));

    // Simulate 30 debounced trades targeting 35 bps moves with Maker entries
    for i in 0..30 {
        gen.advance_time(15_000); // 15-second debounce between entries
        let l2 = gen.generate_l2_update(1.2, 1.5);
        sim.process_event(l2, gen.current_time_ns);

        let bid_px = sim.book.best_bid().unwrap().price;

        // Place resting post-only maker bid
        let buy_intent = OrderIntent {
            client_order_id: (i * 2 + 1) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: bid_px,
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(buy_intent, gen.current_time_ns);

        // Advance 100ms (arrival at exchange)
        gen.advance_time(100);
        let l2_arr = gen.generate_l2_update(1.2, 1.5);
        sim.process_event(l2_arr, gen.current_time_ns);

        // Market trades exhaust queue and fill our maker bid
        let trade = MarketEvent::Trade(TradePrint {
            price: bid_px,
            qty: Fixed8::from_f64(2.5),
            is_buyer_maker: true,
            timestamp_ns: gen.current_time_ns,
        });
        sim.process_event(trade, gen.current_time_ns);

        // Market moves +35 bps in our favor over 60 seconds (structural breakout)
        gen.advance_time(60_000);
        gen.current_mid = gen.current_mid + Fixed8::from_f64(315.0); // +35 bps on $90k
        let l2_target = gen.generate_l2_update(1.2, 1.5);
        sim.process_event(l2_target, gen.current_time_ns);

        // Take-Profit exit (Limit maker sell at ask)
        let ask_px = sim.book.best_ask().unwrap().price;
        let sell_intent = OrderIntent {
            client_order_id: (i * 2 + 2) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: ask_px,
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(sell_intent, gen.current_time_ns);
        gen.advance_time(100);
        let l2_exit_arr = gen.generate_l2_update(1.2, 1.5);
        sim.process_event(l2_exit_arr, gen.current_time_ns);

        // Market fills our TP maker sell
        let exit_trade = MarketEvent::Trade(TradePrint {
            price: ask_px,
            qty: Fixed8::from_f64(2.0),
            is_buyer_maker: false,
            timestamp_ns: gen.current_time_ns,
        });
        sim.process_event(exit_trade, gen.current_time_ns);
    }

    (sim.generate_report(), config)
}

fn run_liquidation_cascade_stress_sim() -> (BacktestReport, BacktestConfig) {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let config = BacktestConfig::tokyo_colocated(); // 1ms latency
    let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, config.clone());
    let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));

    // Setup initial order book
    let l2 = gen.generate_l2_update(1.0, 5.0);
    sim.process_event(l2, gen.current_time_ns);

    // Sniper rests bids 50 bps below mid ($89,550)
    let sniper_bid = Fixed8::from_f64(89_550.0);
    let buy_intent = OrderIntent {
        client_order_id: 9991,
        venue: Venue::Binance,
        symbol: sym,
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GoodTilCancelled,
        price: sniper_bid,
        qty: Fixed8::from_f64(1.0),
        created_at_ns: gen.current_time_ns,
        parent_leg_id: None,
    };
    sim.submit_order(buy_intent, gen.current_time_ns);

    gen.advance_time(2); // 1ms latency elapses, order resting in book

    // Violent $10M liquidation cascade crashes price 80 bps through our bid
    let cascade_events = gen.generate_liquidation_cascade(80.0, 10_000_000.0);
    for evt in cascade_events {
        sim.process_event(evt, gen.current_time_ns);
    }

    // Sniper position is filled at $89,550
    // Market bounces back 50% of the cascade (+40 bps recovery) over 2 minutes
    gen.advance_time(120_000);
    gen.current_mid = gen.current_mid + Fixed8::from_f64(360.0); // +40 bps recovery
    let l2_recovery = gen.generate_l2_update(1.0, 2.0);
    sim.process_event(l2_recovery, gen.current_time_ns);

    // Sniper exits at recovery VWAP
    let _exit_ask = sim.book.best_ask().unwrap().price;
    let sell_intent = OrderIntent {
        client_order_id: 9992,
        venue: Venue::Binance,
        symbol: sym,
        side: Side::Sell,
        order_type: OrderType::ImmediateOrCancel,
        time_in_force: TimeInForce::ImmediateOrCancel,
        price: Fixed8::from_f64(80_000.0),
        qty: Fixed8::from_f64(1.0),
        created_at_ns: gen.current_time_ns,
        parent_leg_id: None,
    };
    sim.submit_order(sell_intent, gen.current_time_ns);
    gen.advance_time(2);
    let l2_exit = gen.generate_l2_update(1.0, 2.0);
    sim.process_event(l2_exit, gen.current_time_ns);

    (sim.generate_report(), config)
}

fn run_seed_calm_time_decay() -> (BacktestReport, BacktestConfig) {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let config = BacktestConfig::remote_mumbai();
    let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, config.clone());
    let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));

    // Seed 1: Calm chop regime where positions timeout after 30s (passive maker time-decay exit)
    for i in 0..15 {
        gen.advance_time(15_000);
        let l2 = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2, gen.current_time_ns);

        let bid_px = sim.book.best_bid().unwrap().price;
        let buy_intent = OrderIntent {
            client_order_id: (i * 2 + 1) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: bid_px,
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(buy_intent, gen.current_time_ns);
        gen.advance_time(100);
        let l2_arr = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_arr, gen.current_time_ns);

        // Fill entry
        let trade = MarketEvent::Trade(TradePrint {
            price: bid_px,
            qty: Fixed8::from_f64(2.0),
            is_buyer_maker: true,
            timestamp_ns: gen.current_time_ns,
        });
        sim.process_event(trade, gen.current_time_ns);

        // Advance 30s in flat chop (price moves +0.5 bps)
        gen.advance_time(30_000);
        gen.current_mid = gen.current_mid + Fixed8::from_f64(4.5);
        let l2_chop = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_chop, gen.current_time_ns);

        // Passive maker time-decay exit at current best ask
        let ask_px = sim.book.best_ask().unwrap().price;
        let sell_intent = OrderIntent {
            client_order_id: (i * 2 + 2) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: ask_px,
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(sell_intent, gen.current_time_ns);
        gen.advance_time(100);
        let l2_exit = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_exit, gen.current_time_ns);

        let exit_trade = MarketEvent::Trade(TradePrint {
            price: ask_px,
            qty: Fixed8::from_f64(2.0),
            is_buyer_maker: false,
            timestamp_ns: gen.current_time_ns,
        });
        sim.process_event(exit_trade, gen.current_time_ns);
    }

    (sim.generate_report(), config)
}

fn run_seed_adverse_stop_loss() -> (BacktestReport, BacktestConfig) {
    let sym = Symbol::from_str_truncate("BTCUSDT");
    let config = BacktestConfig::remote_mumbai();
    let mut sim = FeeAndFillSimulator::new(Venue::Binance, sym, config.clone());
    let mut gen = SyntheticMarketGenerator::new(Venue::Binance, sym, Fixed8::from_f64(90_000.0));

    // Seed 2: Adverse flow regime testing hard IOC taker stop-loss vs toxic market
    for i in 0..10 {
        gen.advance_time(15_000);
        let l2 = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2, gen.current_time_ns);

        let bid_px = sim.book.best_bid().unwrap().price;
        let buy_intent = OrderIntent {
            client_order_id: (i * 2 + 1) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GoodTilCancelled,
            price: bid_px,
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(buy_intent, gen.current_time_ns);
        gen.advance_time(100);
        let l2_arr = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_arr, gen.current_time_ns);

        // Fill entry
        let trade = MarketEvent::Trade(TradePrint {
            price: bid_px,
            qty: Fixed8::from_f64(2.0),
            is_buyer_maker: true,
            timestamp_ns: gen.current_time_ns,
        });
        sim.process_event(trade, gen.current_time_ns);

        // Price drops 6 bps, hitting hard stop-loss threshold
        gen.advance_time(5_000);
        gen.current_mid = gen.current_mid - Fixed8::from_f64(54.0); // -6 bps on $90k
        let l2_drop = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_drop, gen.current_time_ns);

        // HARD TAKER IOC STOP-LOSS (crosses book to guarantee de-risking!)
        let exit_bid = sim.book.best_bid().unwrap().price;
        let sl_intent = OrderIntent {
            client_order_id: (i * 2 + 2) as u64,
            venue: Venue::Binance,
            symbol: sym,
            side: Side::Sell,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: exit_bid,
            qty: Fixed8::from_f64(0.2),
            created_at_ns: gen.current_time_ns,
            parent_leg_id: None,
        };
        sim.submit_order(sl_intent, gen.current_time_ns);
        gen.advance_time(100);
        let l2_sl = gen.generate_l2_update(1.0, 2.0);
        sim.process_event(l2_sl, gen.current_time_ns);
    }

    (sim.generate_report(), config)
}

fn main() {
    println!("\n################################################################################");
    println!("          AERIS INSTITUTIONAL FEE-AND-FILL MICROSTRUCTURE SIMULATION          ");
    println!("################################################################################");

    // Scenario 1: The Raw Soak Trap (Micro-ticks + Taker Fees)
    let (rep1, cfg1) = run_uncalibrated_soak_sim();
    print_simulation_report("SCENARIO 1: THE RAW SOAK TRAP (HYPERACTIVE IOC TAKER)", &rep1, &cfg1);

    // Scenario 2: The Calibrated Edge (15s Debounce + 35 bps Horizon + Maker Orders)
    let (rep2, cfg2) = run_calibrated_maker_sim();
    print_simulation_report("SCENARIO 2: CALIBRATED EDGE (35 BPS HORIZON + MAKER EXECUTION)", &rep2, &cfg2);

    // Scenario 3: Liquidation Cascade Sniper Stress Test
    let (rep3, cfg3) = run_liquidation_cascade_stress_sim();
    print_simulation_report("SCENARIO 3: $10M LIQUIDATION CASCADE WICK SNIPER TEST", &rep3, &cfg3);

    // Multi-Seed Window Replays:
    let (rep_decay, cfg_decay) = run_seed_calm_time_decay();
    print_simulation_report("SEED REPLAY 1: CALM CHOP (PASSIVE MAKER TIME-DECAY EXITS)", &rep_decay, &cfg_decay);

    let (rep_sl, cfg_sl) = run_seed_adverse_stop_loss();
    print_simulation_report("SEED REPLAY 2: ADVERSE MOMENTUM (HARD TAKER IOC STOP-LOSS SAFETY)", &rep_sl, &cfg_sl);

    println!("################################################################################");
    println!("                         SIMULATION COMPARISON SUMMARY                          ");
    println!("################################################################################");
    println!(" Metric                | Scenario 1 (Soak Trap) | Scenario 2 (Calibrated) | Seed 1 (Time-Decay) | Seed 2 (Taker SL)");
    println!("-----------------------+------------------------+-------------------------+---------------------+------------------");
    println!(" Trades Executed       | {:22} | {:23} | {:19} | {:17}", rep1.total_trades, rep2.total_trades, rep_decay.total_trades, rep_sl.total_trades);
    println!(" Gross Realized PnL    | ${:+21.2} | ${:+22.2} | ${:+18.2} | ${:+16.2}", rep1.gross_realized_pnl.to_f64(), rep2.gross_realized_pnl.to_f64(), rep_decay.gross_realized_pnl.to_f64(), rep_sl.gross_realized_pnl.to_f64());
    println!(" Exchange Fees Paid    | -${:21.2} | -${:22.2} | -${:18.2} | -${:16.2}", rep1.total_fees_paid.to_f64(), rep2.total_fees_paid.to_f64(), rep_decay.total_fees_paid.to_f64(), rep_sl.total_fees_paid.to_f64());
    println!(" Net Realized PnL      | ${:+21.2} | ${:+22.2} | ${:+18.2} | ${:+16.2}", rep1.net_realized_pnl.to_f64(), rep2.net_realized_pnl.to_f64(), rep_decay.net_realized_pnl.to_f64(), rep_sl.net_realized_pnl.to_f64());
    println!(" Net Win Rate          | {:21.1}% | {:22.1}% | {:18.1}% | {:16.1}%", rep1.net_win_rate_pct, rep2.net_win_rate_pct, rep_decay.net_win_rate_pct, rep_sl.net_win_rate_pct);
    println!(" Fee/Alpha Burn Ratio  | {:21.2}x | {:22.2}x | {:18.2}x | {:16.2}x", rep1.fee_to_alpha_burn_ratio, rep2.fee_to_alpha_burn_ratio, rep_decay.fee_to_alpha_burn_ratio, rep_sl.fee_to_alpha_burn_ratio);
    println!(" Profit Factor         | {:22.2} | {:23.2} | {:19.2} | {:17.2}", rep1.profit_factor, rep2.profit_factor, rep_decay.profit_factor, rep_sl.profit_factor);
    println!(" Max Drawdown          | {:21.2}% | {:22.2}% | {:18.2}% | {:16.2}%", rep1.max_drawdown_pct, rep2.max_drawdown_pct, rep_decay.max_drawdown_pct, rep_sl.max_drawdown_pct);
    println!("================================================================================\n");
}
