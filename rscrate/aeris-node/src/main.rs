use aeris_core::{
    BookLevel, BookUpdate, Fixed8, OrderIntent, Symbol, TimestampNs, Venue,
};
use aeris_execution::{ExecutionRouter, FrictionModel};
use aeris_feed::{DeadManSnitch, L2OrderBook};
use aeris_ipc::{MacroPayload, PingPongSharedMemory};
use aeris_risk::{
    AccountRiskState, MetaLabelGate, MetaLabelGateConfig, OrphanedLegTracker, PreTradeRiskGateway,
    RiskGatewayConfig,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    pub books: HashMap<Symbol, L2OrderBook>,
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
            .entry(update.symbol)
            .or_insert_with(|| L2OrderBook::new(update.venue, update.symbol));
        book.apply_update(update);
    }

    pub fn execute_cycle(&mut self) -> Vec<OrderIntent> {
        self.cycle_count += 1;
        let now_ns = current_time_ns();

        self.poll_ipc();

        let mut executed_intents = Vec::new();

        let unwinds = self.orphaned_tracker.check_timeouts(now_ns);
        for unwind in unwinds {
            println!(
                "  [ALERT] 50ms Orphaned-Leg Timeout! Executing Emergency IOC Unwind for Order #{}",
                unwind.parent_leg_id.unwrap_or(0)
            );
            executed_intents.push(unwind);
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
            " Macro Bias: {} | IPC Seq: {} | Universe Count: {}",
            match self.active_macro.macro_bias {
                1 => "SUPPLY_SQUEEZE (Bullish)",
                2 => "DISTRIBUTION (Bearish)",
                _ => "NEUTRAL / QUIET",
            },
            self.last_macro_seq,
            self.active_macro.universe_len
        );

        for (sym, book) in &self.books {
            let bbo_bid = book.best_bid().map(|b| b.price.to_f64()).unwrap_or(0.0);
            let bbo_ask = book.best_ask().map(|a| a.price.to_f64()).unwrap_or(0.0);
            let mid = book.mid_price().map(|m| m.to_f64()).unwrap_or(0.0);
            let micro = book.micro_price().map(|m| m.to_f64()).unwrap_or(0.0);
            let div_bps = book
                .micro_price_divergence_bps()
                .map(|d| d.to_f64())
                .unwrap_or(0.0);

            println!(
                "   [{}] Bid: ${:.2} | Ask: ${:.2} | Mid: ${:.2} | Micro: ${:.2} (Div: {:+.2} bps)",
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

    let shm_path = PathBuf::from(
        "D:/Pushp - Do not open without permission/AERIS/Python Playground/data/aeris_shm.dat",
    );
    let mut node = AerisEngineNode::new(Fixed8::from_f64(100_000.0), Some(shm_path));

    let btc_sym = Symbol::from_str_truncate("BTCUSDT");
    let avax_sym = Symbol::from_str_truncate("AVAXUSDT");

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        r.store(false, Ordering::SeqCst);
    });

    println!("[INFO] Initialized node with $100,000.00 equity. Starting tick loop...");

    let mut tick_counter: u64 = 0;

    while running.load(Ordering::SeqCst) && tick_counter < 5 {
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

    println!("[INFO] Node simulation completed successfully.");
}
