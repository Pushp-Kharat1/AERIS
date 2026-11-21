use aeris_core::{
    BookLevel, BookUpdate, Fixed8, OrderIntent, Symbol, TimestampNs, Venue,
};
use aeris_execution::{ExecutionRouter, FrictionModel};
use aeris_feed::{
    connect_binance_depth, connect_bybit_depth, DeadManSnitch, L2OrderBook,
};
use aeris_ipc::{MacroPayload, PingPongSharedMemory};
use aeris_risk::{
    AccountRiskState, MetaLabelGate, MetaLabelGateConfig, OrphanedLegTracker, PreTradeRiskGateway,
    RiskGatewayConfig,
};
use aeris_strategy::{CrossVenueArbEngine, CrossVenueOpportunity, MicroMomentumEngine};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

fn current_time_ns() -> TimestampNs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

pub struct AerisEngineNode {
    pub account: AccountRiskState,
    pub risk_gateway: PreTradeRiskGateway,
    pub meta_gate: MetaLabelGate,
    pub router: ExecutionRouter,
    pub orphaned_tracker: OrphanedLegTracker,
    pub deadman: DeadManSnitch,
    pub books: HashMap<(Venue, Symbol), L2OrderBook>,
    pub arb_engine: CrossVenueArbEngine,
    pub momentum_engine: MicroMomentumEngine,
    pub last_cross_venue_opp: Option<CrossVenueOpportunity>,
    pub total_spreads_executed: u64,
    pub shm: Option<PingPongSharedMemory>,
    pub last_macro_seq: u64,
    pub active_macro: MacroPayload,
    pub total_fees_paid: Fixed8,
    pub total_tds_paid: Fixed8,
    pub cycle_count: u64,
}

impl AerisEngineNode {
    pub fn new(initial_equity: Fixed8, shm_path: Option<PathBuf>) -> Self {
        let risk_config = RiskGatewayConfig {
            max_leverage: Fixed8::from_f64(2.5),
            max_order_notional: Fixed8::from_f64(100_000.0),
            max_price_deviation_bps: 300,
        };

        let risk_gateway = PreTradeRiskGateway::new(risk_config);
        let meta_gate = MetaLabelGate::new(MetaLabelGateConfig::default());
        let friction = FrictionModel::default();
        let router = ExecutionRouter::new(risk_gateway.clone(), friction);

        let arb_engine = CrossVenueArbEngine::new(
            Fixed8::from_f64(2.0), // 2.0 bps minimum net profit hurdle after fees + TDS
            Fixed8::from_f64(50_000.0), // $50,000 max single leg notional
        );
        let momentum_engine = MicroMomentumEngine::new(
            Fixed8::from_f64(0.5), // 0.5 bps divergence threshold
            Fixed8::from_f64(0.1), // 0.1 BTC order size
        );

        let shm = shm_path.and_then(|p| PingPongSharedMemory::create_or_open(p).ok());

        Self {
            account: AccountRiskState {
                total_equity: initial_equity,
                total_deployed_notional: Fixed8::ZERO,
            },
            risk_gateway,
            meta_gate,
            router,
            orphaned_tracker: OrphanedLegTracker::new(50), // 50ms timeout
            deadman: DeadManSnitch::new(3000),             // 3s timeout
            books: HashMap::new(),
            arb_engine,
            momentum_engine,
            last_cross_venue_opp: None,
            total_spreads_executed: 0,
            shm,
            last_macro_seq: 0,
            active_macro: MacroPayload::default(),
            total_fees_paid: Fixed8::ZERO,
            total_tds_paid: Fixed8::ZERO,
            cycle_count: 0,
        }
    }

    pub fn poll_ipc(&mut self) -> bool {
        if let Some(ref shm) = self.shm {
            let (seq, payload) = shm.read_active();
            if seq > self.last_macro_seq {
                self.last_macro_seq = seq;
                self.active_macro = payload;
                return true;
            }
        }
        false
    }

    pub fn on_book_update(&mut self, update: &BookUpdate) {
        let now_ns = current_time_ns();
        self.deadman.record_heartbeat(update.venue, now_ns);

        let book = self
            .books
            .entry((update.venue, update.symbol))
            .or_insert_with(|| L2OrderBook::new(update.venue, update.symbol));
        book.apply_update(update);
    }

    pub fn execute_cycle(&mut self) -> Vec<OrderIntent> {
        self.cycle_count += 1;
        let now_ns = current_time_ns();

        self.poll_ipc();

        let mut executed_intents = Vec::new();

        // 1. Orphaned-Leg 50ms Timeout Check
        let unwinds = self.orphaned_tracker.check_timeouts(now_ns);
        for unwind in unwinds {
            println!(
                "  [ALERT] 50ms Orphaned-Leg Timeout! Executing Emergency IOC Unwind for Order #{}",
                unwind.parent_leg_id.unwrap_or(0)
            );
            executed_intents.push(unwind);
        }

        // 2. Cross-Venue Net Spread Arbitrage Scanner
        let unique_symbols: HashSet<Symbol> = self.books.keys().map(|(_, sym)| *sym).collect();
        for sym in unique_symbols {
            let binance_book = self.books.get(&(Venue::Binance, sym));
            let bybit_book = self.books.get(&(Venue::Bybit, sym));

            if let (Some(b_book), Some(by_book)) = (binance_book, bybit_book) {
                if let Some(opp) = self.arb_engine.inspect_spread(sym, b_book, by_book) {
                    self.last_cross_venue_opp = Some(opp.clone());

                    if let Some(spread) = self.arb_engine.generate_spread_intent(&opp, now_ns) {
                        println!(
                            "  [STRATEGY] 🔥 ARB OPPORTUNITY: Buy {:?} @ ${:.2} | Sell {:?} @ ${:.2} | Gross: {:+.2} bps | Net: {:+.2} bps | Qty: {}",
                            opp.buy_venue,
                            opp.buy_price.to_f64(),
                            opp.sell_venue,
                            opp.sell_price.to_f64(),
                            opp.gross_spread_bps.to_f64(),
                            opp.net_spread_bps.to_f64(),
                            opp.max_qty.to_f64()
                        );

                        let risk_a = self.risk_gateway.evaluate_order(&spread.leg_a_buy, &self.account, Some(opp.buy_price));
                        let risk_b = self.risk_gateway.evaluate_order(&spread.leg_b_sell, &self.account, Some(opp.sell_price));

                        if risk_a.is_ok() && risk_b.is_ok() {
                            self.orphaned_tracker.register_spread(
                                spread.spread_id,
                                spread.leg_a_buy,
                                spread.leg_b_sell,
                            );

                            let _ = self.router.validate_and_route(&spread.leg_a_buy, &self.account, Some(opp.buy_price));
                            let _ = self.router.validate_and_route(&spread.leg_b_sell, &self.account, Some(opp.sell_price));

                            self.total_spreads_executed += 1;
                            executed_intents.push(spread.leg_a_buy);
                            executed_intents.push(spread.leg_b_sell);
                        }
                    }
                }
            }
        }

        // 3. Stoikov Micro-Divergence Momentum Scanner
        for ((_, _), book) in &self.books {
            if let Some(intent) = self.momentum_engine.evaluate_signal(book, self.active_macro.macro_bias, now_ns) {
                if self.risk_gateway.evaluate_order(&intent, &self.account, Some(intent.price)).is_ok() {
                    let _ = self.router.validate_and_route(&intent, &self.account, Some(intent.price));
                    executed_intents.push(intent);
                }
            }
        }

        executed_intents
    }

    pub fn print_status(&self) {
        let leverage = if !self.account.total_equity.is_zero() {
            self.account.total_deployed_notional / self.account.total_equity
        } else {
            Fixed8::ZERO
        };

        println!("--------------------------------------------------------------------------------");
        println!(
            " AERIS RUST NODE [Cycle #{}] | Equity: ${:.2} | Deployed: ${:.2} | Leverage: {:.2}x",
            self.cycle_count,
            self.account.total_equity.to_f64(),
            self.account.total_deployed_notional.to_f64(),
            leverage.to_f64()
        );
        println!(
            " Macro Bias: {} | IPC Seq: {} | Universe Count: {} | Total Spreads Executed: {}",
            match self.active_macro.macro_bias {
                1 => "SUPPLY_SQUEEZE (Bullish)",
                2 => "DISTRIBUTION (Bearish)",
                _ => "NEUTRAL / QUIET",
            },
            self.last_macro_seq,
            self.active_macro.universe_len,
            self.total_spreads_executed
        );

        if let Some(ref opp) = self.last_cross_venue_opp {
            println!(
                "   [ARB SPREAD] {} Buy {:?} (${:.2}) -> Sell {:?} (${:.2}) | Gross: {:+.2} bps | Net: {:+.2} bps",
                opp.symbol.as_str(),
                opp.buy_venue,
                opp.buy_price.to_f64(),
                opp.sell_venue,
                opp.sell_price.to_f64(),
                opp.gross_spread_bps.to_f64(),
                opp.net_spread_bps.to_f64()
            );
        }

        for ((venue, sym), book) in &self.books {
            let bbo_bid = book.best_bid().map(|b| b.price.to_f64()).unwrap_or(0.0);
            let bbo_ask = book.best_ask().map(|a| a.price.to_f64()).unwrap_or(0.0);
            let mid = book.mid_price().map(|m| m.to_f64()).unwrap_or(0.0);
            let micro = book.micro_price().map(|m| m.to_f64()).unwrap_or(0.0);
            let div_bps = book
                .micro_price_divergence_bps()
                .map(|d| d.to_f64())
                .unwrap_or(0.0);

            println!(
                "   [{:?} {}] Bid: ${:.2} | Ask: ${:.2} | Mid: ${:.2} | Micro: ${:.2} (Div: {:+.2} bps)",
                venue,
                sym.as_str(),
                bbo_bid,
                bbo_ask,
                mid,
                micro,
                div_bps
            );
        }
        println!("--------------------------------------------------------------------------------");
    }
}

#[tokio::main]
async fn main() {
    println!("\n================================================================================");
    println!("             AERIS PRODUCTION ENGINE NODE (ARMORED RUST RUNTIME)                ");
    println!("================================================================================\n");

    let args: Vec<String> = std::env::args().collect();
    let is_live = args.iter().any(|a| a == "--live");
    let max_ticks: Option<u64> = args
        .iter()
        .position(|a| a == "--ticks")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok());

    let shm_path = PathBuf::from(
        "D:/Pushp - Do not open without permission/AERIS/Python Playground/data/aeris_shm.dat",
    );
    let mut node = AerisEngineNode::new(Fixed8::from_f64(100_000.0), Some(shm_path));

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\n[INFO] Shutdown signal received. Stopping Aeris Engine Node...");
        r.store(false, Ordering::SeqCst);
    });

    if is_live {
        println!("[INFO] Initialized node with $100,000.00 equity in LIVE STREAMING mode.");
        let (tx, mut rx) = mpsc::channel::<BookUpdate>(1024);

        let tx_binance = tx.clone();
        tokio::spawn(async move {
            println!("[FEED] Spawning Binance WebSocket feed handler for BTCUSDT...");
            if let Err(e) = connect_binance_depth("BTCUSDT", tx_binance).await {
                eprintln!("[WARN] Binance WS terminated: {:?}", e);
            }
        });

        let tx_bybit = tx.clone();
        tokio::spawn(async move {
            println!("[FEED] Spawning Bybit WebSocket feed handler for BTCUSDT...");
            if let Err(e) = connect_bybit_depth("BTCUSDT", tx_bybit).await {
                eprintln!("[WARN] Bybit WS terminated: {:?}", e);
            }
        });

        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut ticks: u64 = 0;

        while running.load(Ordering::SeqCst) {
            tokio::select! {
                biased;
                Some(update) = rx.recv() => {
                    node.on_book_update(&update);
                    println!(
                        "  -> [LIVE FEED] {:?} {} | Best Bid: ${:.2} | Best Ask: ${:.2}",
                        update.venue,
                        update.symbol.as_str(),
                        update.bids.first().map(|b| b.price.to_f64()).unwrap_or(0.0),
                        update.asks.first().map(|a| a.price.to_f64()).unwrap_or(0.0)
                    );
                }
                _ = interval.tick() => {
                    node.execute_cycle();
                    node.print_status();
                    ticks += 1;
                    if let Some(limit) = max_ticks {
                        if ticks >= limit {
                            break;
                        }
                    }
                }
            }
        }
    } else {
        println!("[INFO] Initialized node with $100,000.00 equity in SIMULATION mode.");
        let btc_sym = Symbol::from_str_truncate("BTCUSDT");
        let avax_sym = Symbol::from_str_truncate("AVAXUSDT");

        let mut tick_counter: u64 = 0;
        let limit = max_ticks.unwrap_or(5);

        while running.load(Ordering::SeqCst) && tick_counter < limit {
            tick_counter += 1;

            let btc_bid = 60_000.0 + (tick_counter as f64 * 0.5);
            let btc_update = BookUpdate {
                venue: Venue::Binance,
                symbol: btc_sym,
                timestamp_ns: current_time_ns(),
                bids: vec![
                    BookLevel {
                        price: Fixed8::from_f64(btc_bid),
                        qty: Fixed8::from_f64(12.5),
                    },
                    BookLevel {
                        price: Fixed8::from_f64(btc_bid - 5.0),
                        qty: Fixed8::from_f64(20.0),
                    },
                ],
                asks: vec![
                    BookLevel {
                        price: Fixed8::from_f64(btc_bid + 5.0),
                        qty: Fixed8::from_f64(2.0),
                    },
                    BookLevel {
                        price: Fixed8::from_f64(btc_bid + 10.0),
                        qty: Fixed8::from_f64(5.0),
                    },
                ],
                is_snapshot: true,
            };
            node.on_book_update(&btc_update);

            let bybit_btc_ask = btc_bid + 2.0;
            let bybit_btc_bid = btc_bid - 3.0;
            let bybit_btc_update = BookUpdate {
                venue: Venue::Bybit,
                symbol: btc_sym,
                timestamp_ns: current_time_ns(),
                bids: vec![BookLevel {
                    price: Fixed8::from_f64(bybit_btc_bid),
                    qty: Fixed8::from_f64(8.0),
                }],
                asks: vec![BookLevel {
                    price: Fixed8::from_f64(bybit_btc_ask),
                    qty: Fixed8::from_f64(10.0),
                }],
                is_snapshot: true,
            };
            node.on_book_update(&bybit_btc_update);

            let avax_bid = 30.0 + (tick_counter as f64 * 0.02);
            let avax_update = BookUpdate {
                venue: Venue::Bybit,
                symbol: avax_sym,
                timestamp_ns: current_time_ns(),
                bids: vec![BookLevel {
                    price: Fixed8::from_f64(avax_bid),
                    qty: Fixed8::from_f64(500.0),
                }],
                asks: vec![BookLevel {
                    price: Fixed8::from_f64(avax_bid + 0.05),
                    qty: Fixed8::from_f64(150.0),
                }],
                is_snapshot: true,
            };
            node.on_book_update(&avax_update);

            node.execute_cycle();
            node.print_status();

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    println!("[INFO] Node execution completed cleanly.");
}
