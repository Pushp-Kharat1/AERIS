use aeris_core::{
    BookLevel, BookUpdate, Fixed8, OrderIntent, Side, Symbol, TimestampNs, Venue,
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
use aeris_strategy::{
    AggressiveBreakoutEngine, CounterPunchEngine, CrossVenueArbEngine, CrossVenueOpportunity,
    MicroMomentumEngine,
};
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StrategyId {
    StoikovRaw,
    StoikovCalibrated,
    BreakoutRaw,
    BreakoutCalibrated,
    CounterPunchRaw,
    CounterPunchCalibrated,
    CrossVenueArb,
}

impl StrategyId {
    pub fn name(&self) -> &'static str {
        match self {
            StrategyId::StoikovRaw => "StoikovRaw",
            StrategyId::StoikovCalibrated => "StoikovCalib",
            StrategyId::BreakoutRaw => "BreakoutRaw",
            StrategyId::BreakoutCalibrated => "BreakoutCalib",
            StrategyId::CounterPunchRaw => "CounterPunchRaw",
            StrategyId::CounterPunchCalibrated => "CounterPunchCalib",
            StrategyId::CrossVenueArb => "CrossVenueArb",
        }
    }

    pub fn is_calibrated(&self) -> bool {
        matches!(
            self,
            StrategyId::StoikovCalibrated
                | StrategyId::BreakoutCalibrated
                | StrategyId::CounterPunchCalibrated
        )
    }

    pub fn cohort(&self) -> &'static str {
        if self.is_calibrated() {
            "CALIBRATED"
        } else if *self == StrategyId::CrossVenueArb {
            "ARBITRAGE"
        } else {
            "OLD CHAPS"
        }
    }
}

#[derive(Clone, Debug)]
pub struct StrategyPosition {
    pub strategy: StrategyId,
    pub venue: Venue,
    pub symbol: Symbol,
    pub side: Side,
    pub qty: Fixed8,
    pub avg_entry_price: Fixed8,
    pub total_notional: Fixed8,
    pub entry_time_ns: TimestampNs,
    pub take_profit_price: Fixed8,
    pub stop_loss_price: Fixed8,
    pub entry_cycle: u64,
    pub entry_fee: Fixed8,
    pub entry_tds: Fixed8,
}

#[derive(Clone, Debug, Default)]
pub struct StrategyMetrics {
    pub fills_count: u64,
    pub closed_trades: u64,
    pub winning_trades: u64,
    pub gross_winning_trades: u64,
    pub internal_crosses_count: u64,
    pub fees_saved: Fixed8,
    pub gross_realized_pnl: Fixed8,
    pub fees_paid: Fixed8,
    pub closed_fees_paid: Fixed8,
    pub tds_paid: Fixed8,
    pub closed_tds_paid: Fixed8,
    pub net_realized_pnl: Fixed8,
}

#[derive(Clone, Debug)]
pub struct StrategyCandidate {
    pub strategy: StrategyId,
    pub intent: OrderIntent,
    pub take_profit_price: Fixed8,
    pub stop_loss_price: Fixed8,
}

pub struct AerisEngineNode {
    pub initial_equity: Fixed8,
    pub account: AccountRiskState,
    pub realized_pnl: Fixed8,
    pub paper_positions: HashMap<(StrategyId, Venue, Symbol), StrategyPosition>,
    pub strategy_metrics: HashMap<StrategyId, StrategyMetrics>,
    pub recent_mids: HashMap<(Venue, Symbol), Vec<Fixed8>>,
    pub total_fills_count: u64,
    pub total_internal_crosses: u64,
    pub total_fees_saved: Fixed8,
    pub risk_gateway: PreTradeRiskGateway,
    pub meta_gate: MetaLabelGate,
    pub friction: FrictionModel,
    pub router: ExecutionRouter,
    pub orphaned_tracker: OrphanedLegTracker,
    pub deadman: DeadManSnitch,
    pub books: HashMap<(Venue, Symbol), L2OrderBook>,
    pub arb_engine: CrossVenueArbEngine,
    pub raw_momentum_engine: MicroMomentumEngine,
    pub calib_momentum_engine: MicroMomentumEngine,
    pub raw_breakout_engine: AggressiveBreakoutEngine,
    pub calib_breakout_engine: AggressiveBreakoutEngine,
    pub raw_counter_punch_engine: CounterPunchEngine,
    pub calib_counter_punch_engine: CounterPunchEngine,
    pub last_cross_venue_opp: Option<CrossVenueOpportunity>,
    pub total_spreads_executed: u64,
    pub shm: Option<PingPongSharedMemory>,
    pub last_macro_seq: u64,
    pub active_macro: MacroPayload,
    pub total_fees_paid: Fixed8,
    pub total_tds_paid: Fixed8,
    pub is_onshore: bool,
    pub cycle_count: u64,
    pub last_entry_time_ns: HashMap<(StrategyId, Venue, Symbol), TimestampNs>,
}

impl AerisEngineNode {
    pub fn new(initial_equity: Fixed8, is_onshore: bool, shm_path: Option<PathBuf>) -> Self {
        let risk_config = RiskGatewayConfig {
            max_leverage: Fixed8::from_f64(2.5),
            max_order_notional: Fixed8::from_f64(100_000.0),
            max_price_deviation_bps: 300,
        };

        let risk_gateway = PreTradeRiskGateway::new(risk_config);
        let meta_gate = MetaLabelGate::new(MetaLabelGateConfig::default());
        let friction = if is_onshore {
            FrictionModel::default()
        } else {
            FrictionModel::offshore_linear()
        };
        let router = ExecutionRouter::new(risk_gateway.clone(), friction);

        let arb_engine = CrossVenueArbEngine::new(
            Fixed8::from_f64(2.0),
            Fixed8::from_f64(50_000.0),
        );
        let raw_momentum_engine = MicroMomentumEngine::new(
            Fixed8::from_f64(0.005), // 0.005 bps divergence threshold (Old Chap)
            Fixed8::from_f64(0.05),
        );
        let calib_momentum_engine = MicroMomentumEngine::new(
            Fixed8::from_f64(0.12),  // 0.12 bps divergence threshold (Calibrated Stoikov)
            Fixed8::from_f64(0.05),
        );
        let raw_breakout_engine = AggressiveBreakoutEngine::new(
            Fixed8::from_f64(0.3),   // 0.3 bps breakout threshold (Old Chap)
            Fixed8::from_f64(0.05),
        );
        let calib_breakout_engine = AggressiveBreakoutEngine::new(
            Fixed8::from_f64(1.5),   // 1.5 bps breakout threshold (Calibrated)
            Fixed8::from_f64(0.05),
        );
        let raw_counter_punch_engine = CounterPunchEngine::new(
            Fixed8::from_f64(0.3),   // 0.3 bps stretch threshold (Old Chap)
            Fixed8::from_f64(0.05),
        );
        let calib_counter_punch_engine = CounterPunchEngine::new(
            Fixed8::from_f64(1.0),   // 1.0 bps stretch threshold (Calibrated)
            Fixed8::from_f64(0.05),
        );

        let shm = shm_path.and_then(|p| PingPongSharedMemory::create_or_open(p).ok());

        Self {
            initial_equity,
            account: AccountRiskState {
                total_equity: initial_equity,
                total_deployed_notional: Fixed8::ZERO,
            },
            realized_pnl: Fixed8::ZERO,
            paper_positions: HashMap::new(),
            strategy_metrics: HashMap::new(),
            recent_mids: HashMap::new(),
            total_fills_count: 0,
            total_internal_crosses: 0,
            total_fees_saved: Fixed8::ZERO,
            risk_gateway,
            meta_gate,
            friction,
            router,
            orphaned_tracker: OrphanedLegTracker::new(50),
            deadman: DeadManSnitch::new(3000),
            books: HashMap::new(),
            arb_engine,
            raw_momentum_engine,
            calib_momentum_engine,
            raw_breakout_engine,
            calib_breakout_engine,
            raw_counter_punch_engine,
            calib_counter_punch_engine,
            last_cross_venue_opp: None,
            total_spreads_executed: 0,
            shm,
            last_macro_seq: 0,
            active_macro: MacroPayload::default(),
            total_fees_paid: Fixed8::ZERO,
            total_tds_paid: Fixed8::ZERO,
            is_onshore,
            cycle_count: 0,
            last_entry_time_ns: HashMap::new(),
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

    pub fn apply_strategy_fill(
        &mut self,
        strategy: StrategyId,
        intent: &OrderIntent,
        fill_price: Fixed8,
        fill_qty: Fixed8,
        is_maker: bool,
        is_internal_cross: bool,
        take_profit_price: Fixed8,
        stop_loss_price: Fixed8,
        now_ns: TimestampNs,
    ) {
        let (fee_paid, tds_paid) = if is_internal_cross {
            (Fixed8::ZERO, Fixed8::ZERO)
        } else {
            let report = self.router.process_fill(intent, fill_price, fill_qty, is_maker, now_ns);
            (report.fee_paid, report.tds_paid)
        };

        self.total_fees_paid = self.total_fees_paid + fee_paid;
        self.total_tds_paid = self.total_tds_paid + tds_paid;
        self.total_fills_count += 1;
        self.account.total_equity = self.account.total_equity - fee_paid - tds_paid;

        let m = self.strategy_metrics.entry(strategy).or_default();
        m.fills_count += 1;
        if is_internal_cross {
            m.internal_crosses_count += 1;
        }
        m.fees_paid = m.fees_paid + fee_paid;
        m.tds_paid = m.tds_paid + tds_paid;

        let key = (strategy, intent.venue, intent.symbol);
        if let Some(pos) = self.paper_positions.get_mut(&key) {
            if pos.side == intent.side {
                let existing_notional = pos.avg_entry_price * pos.qty;
                let new_notional = fill_price * fill_qty;
                pos.qty = pos.qty + fill_qty;
                pos.total_notional = existing_notional + new_notional;
                pos.avg_entry_price = pos.total_notional / pos.qty;
                pos.take_profit_price = take_profit_price;
                pos.stop_loss_price = stop_loss_price;
                pos.entry_fee = pos.entry_fee + fee_paid;
                pos.entry_tds = pos.entry_tds + tds_paid;
            }
        } else {
            self.paper_positions.insert(
                key,
                StrategyPosition {
                    strategy,
                    venue: intent.venue,
                    symbol: intent.symbol,
                    side: intent.side,
                    qty: fill_qty,
                    avg_entry_price: fill_price,
                    total_notional: fill_price * fill_qty,
                    entry_time_ns: now_ns,
                    take_profit_price,
                    stop_loss_price,
                    entry_cycle: self.cycle_count,
                    entry_fee: fee_paid,
                    entry_tds: tds_paid,
                },
            );
        }

        self.last_entry_time_ns.insert((strategy, intent.venue, intent.symbol), now_ns);

        self.account.total_deployed_notional = self
            .paper_positions
            .values()
            .fold(Fixed8::ZERO, |acc, p| acc + p.total_notional);

        if is_internal_cross {
            println!(
                "  [DARK POOL FILL] ⚡ {:?} CROSS-FILLED {:?} {:?} {:.4} @ ${:.2} (ZERO EXCHANGE FEES!)",
                strategy,
                intent.venue,
                intent.side,
                fill_qty.to_f64(),
                fill_price.to_f64(),
            );
        } else {
            println!(
                "  [STRATEGY ENTRY] ⚡ {:?} FILLED {:?} {:?} {:.4} @ ${:.2} (TP: ${:.2} | SL: ${:.2})",
                strategy,
                intent.venue,
                intent.side,
                fill_qty.to_f64(),
                fill_price.to_f64(),
                take_profit_price.to_f64(),
                stop_loss_price.to_f64()
            );
        }
    }

    pub fn evaluate_position_exits(&mut self, _now_ns: TimestampNs) {
        let mut exits = Vec::new();

        for (key, pos) in &self.paper_positions {
            if let Some(book) = self.books.get(&(pos.venue, pos.symbol)) {
                match pos.side {
                    Side::Buy => {
                        if let Some(bid) = book.best_bid() {
                            let is_tp = bid.price >= pos.take_profit_price;
                            let is_sl = bid.price <= pos.stop_loss_price;
                            let timeout_cycles = if pos.strategy.is_calibrated() { 60 } else { 16 };
                            let is_timeout = (self.cycle_count.saturating_sub(pos.entry_cycle)) >= timeout_cycles;
                            if is_tp || is_sl || is_timeout {
                                let reason = if is_tp {
                                    "TAKE-PROFIT"
                                } else if is_sl {
                                    "STOP-LOSS"
                                } else {
                                    "TIME-DECAY"
                                };
                                exits.push((*key, pos.clone(), bid.price, reason));
                            }
                        }
                    }
                    Side::Sell => {
                        if let Some(ask) = book.best_ask() {
                            let is_tp = ask.price <= pos.take_profit_price;
                            let is_sl = ask.price >= pos.stop_loss_price;
                            let timeout_cycles = if pos.strategy.is_calibrated() { 60 } else { 16 };
                            let is_timeout = (self.cycle_count.saturating_sub(pos.entry_cycle)) >= timeout_cycles;
                            if is_tp || is_sl || is_timeout {
                                let reason = if is_tp {
                                    "TAKE-PROFIT"
                                } else if is_sl {
                                    "STOP-LOSS"
                                } else {
                                    "TIME-DECAY"
                                };
                                exits.push((*key, pos.clone(), ask.price, reason));
                            }
                        }
                    }
                }
            }
        }

        for (key, pos, exit_price, reason) in exits {
            self.paper_positions.remove(&key);

            let exit_side = match pos.side {
                Side::Buy => Side::Sell,
                Side::Sell => Side::Buy,
            };

            let notional = exit_price * pos.qty;
            // Calibrated: Passive maker for Take-Profit and Time-Decay; Hard aggressive taker for Stop-Loss!
            let is_maker_exit = pos.strategy.is_calibrated() && (reason == "TAKE-PROFIT" || reason == "TIME-DECAY");
            let exit_fees = self.friction.compute_fee(notional, is_maker_exit);
            let exit_tds = self.friction.compute_tds(notional, exit_side);

            let gross_pnl = match pos.side {
                Side::Buy => (exit_price - pos.avg_entry_price) * pos.qty,
                Side::Sell => (pos.avg_entry_price - exit_price) * pos.qty,
            };

            let roundtrip_fees = pos.entry_fee + exit_fees;
            let roundtrip_tds = pos.entry_tds + exit_tds;
            let net_pnl = gross_pnl - roundtrip_fees - roundtrip_tds;

            self.realized_pnl = self.realized_pnl + gross_pnl;
            // account.total_equity already had pos.entry_fee debited at entry; now apply gross_pnl - exit_fees - exit_tds
            self.account.total_equity = self.account.total_equity + gross_pnl - exit_fees - exit_tds;
            self.total_fees_paid = self.total_fees_paid + exit_fees;
            self.total_tds_paid = self.total_tds_paid + exit_tds;

            let m = self.strategy_metrics.entry(pos.strategy).or_default();
            m.closed_trades += 1;
            if gross_pnl.is_positive() {
                m.gross_winning_trades += 1;
            }
            if net_pnl.is_positive() {
                m.winning_trades += 1;
            }
            m.gross_realized_pnl = m.gross_realized_pnl + gross_pnl;
            m.fees_paid = m.fees_paid + exit_fees;
            m.closed_fees_paid = m.closed_fees_paid + roundtrip_fees;
            m.tds_paid = m.tds_paid + exit_tds;
            m.closed_tds_paid = m.closed_tds_paid + roundtrip_tds;
            m.net_realized_pnl = m.net_realized_pnl + net_pnl;

            println!(
                "  [STRATEGY EXIT] 🎯 {:?} triggered {} on {:?} {} | Entry: ${:.2} -> Exit: ${:.2} | Gross: ${:+.4} | Fees: -${:.4} | Net: ${:+.4}",
                pos.strategy,
                reason,
                pos.venue,
                pos.symbol.as_str(),
                pos.avg_entry_price.to_f64(),
                exit_price.to_f64(),
                gross_pnl.to_f64(),
                roundtrip_fees.to_f64(),
                net_pnl.to_f64()
            );
        }

        self.account.total_deployed_notional = self
            .paper_positions
            .values()
            .fold(Fixed8::ZERO, |acc, p| acc + p.total_notional);
    }

    pub fn compute_unrealized_pnl(&self) -> Fixed8 {
        let mut total_unrealized = Fixed8::ZERO;
        for ((_, venue, sym), pos) in &self.paper_positions {
            if let Some(book) = self.books.get(&(*venue, *sym)) {
                let exit_px = match pos.side {
                    Side::Buy => book.best_bid().map(|b| b.price),
                    Side::Sell => book.best_ask().map(|a| a.price),
                };
                if let Some(px) = exit_px {
                    let gross = match pos.side {
                        Side::Buy => (px - pos.avg_entry_price) * pos.qty,
                        Side::Sell => (pos.avg_entry_price - px) * pos.qty,
                    };
                    let notional = px * pos.qty;
                    let est_fee = self.friction.compute_fee(notional, false);
                    let exit_side = match pos.side {
                        Side::Buy => Side::Sell,
                        Side::Sell => Side::Buy,
                    };
                    let est_tds = self.friction.compute_tds(notional, exit_side);
                    total_unrealized = total_unrealized + gross - est_fee - est_tds;
                }
            }
        }
        total_unrealized
    }

    pub fn on_book_update(&mut self, update: &BookUpdate) {
        let now_ns = current_time_ns();
        self.deadman.record_heartbeat(update.venue, now_ns);

        let book = self
            .books
            .entry((update.venue, update.symbol))
            .or_insert_with(|| L2OrderBook::new(update.venue, update.symbol));
        book.apply_update(update);

        if let Some(mid) = book.mid_price() {
            let hist = self.recent_mids.entry((update.venue, update.symbol)).or_default();
            hist.push(mid);
            if hist.len() > 20 {
                hist.remove(0);
            }
        }
    }

    pub fn execute_cycle(&mut self) -> Vec<OrderIntent> {
        self.cycle_count += 1;
        let now_ns = current_time_ns();

        self.poll_ipc();

        // 1. Evaluate open position exits (Take-Profit / Stop-Loss / Timeout)
        self.evaluate_position_exits(now_ns);

        let mut executed_intents = Vec::new();

        // 2. Orphaned-Leg 50ms Timeout Check
        let unwinds = self.orphaned_tracker.check_timeouts(now_ns);
        for unwind in unwinds {
            println!(
                "  [ALERT] 50ms Orphaned-Leg Timeout! Executing Emergency IOC Unwind for Order #{}",
                unwind.parent_leg_id.unwrap_or(0)
            );
            if let Some(book) = self.books.get(&(unwind.venue, unwind.symbol)) {
                let fill_px = match unwind.side {
                    Side::Buy => book.best_ask().map(|a| a.price).unwrap_or(unwind.price),
                    Side::Sell => book.best_bid().map(|b| b.price).unwrap_or(unwind.price),
                };
                self.apply_strategy_fill(
                    StrategyId::CrossVenueArb,
                    &unwind,
                    fill_px,
                    unwind.qty,
                    false,
                    false,
                    fill_px,
                    fill_px,
                    now_ns,
                );
            }
            executed_intents.push(unwind);
        }

        // 3. Strategy: Cross-Venue Net Spread Arbitrage Scanner
        let unique_symbols: HashSet<Symbol> = self.books.keys().map(|(_, sym)| *sym).collect();
        for sym in unique_symbols {
            let binance_book = self.books.get(&(Venue::Binance, sym)).cloned();
            let bybit_book = self.books.get(&(Venue::Bybit, sym)).cloned();

            if let (Some(b_book), Some(by_book)) = (binance_book, bybit_book) {
                if let Some(opp) = self.arb_engine.inspect_spread(sym, &b_book, &by_book) {
                    self.last_cross_venue_opp = Some(opp.clone());

                    if let Some(spread) = self.arb_engine.generate_spread_intent(&opp, now_ns) {
                        println!(
                            "  [STRATEGY] 🔥 ARB OPPORTUNITY: Buy {:?} @ ${:.2} | Sell {:?} @ ${:.2} | Gross: {:+.2} bps | Net: {:+.2} bps",
                            opp.buy_venue,
                            opp.buy_price.to_f64(),
                            opp.sell_venue,
                            opp.sell_price.to_f64(),
                            opp.gross_spread_bps.to_f64(),
                            opp.net_spread_bps.to_f64()
                        );

                        let risk_a = self.risk_gateway.evaluate_order(&spread.leg_a_buy, &self.account, Some(opp.buy_price));
                        let risk_b = self.risk_gateway.evaluate_order(&spread.leg_b_sell, &self.account, Some(opp.sell_price));

                        if risk_a.is_ok() && risk_b.is_ok() {
                            self.orphaned_tracker.register_spread(
                                spread.spread_id,
                                spread.leg_a_buy,
                                spread.leg_b_sell,
                            );

                            self.apply_strategy_fill(
                                StrategyId::CrossVenueArb,
                                &spread.leg_a_buy,
                                opp.buy_price,
                                spread.executable_qty,
                                false,
                                false,
                                opp.buy_price * Fixed8::from_f64(1.0005),
                                opp.buy_price * Fixed8::from_f64(0.9995),
                                now_ns,
                            );
                            self.apply_strategy_fill(
                                StrategyId::CrossVenueArb,
                                &spread.leg_b_sell,
                                opp.sell_price,
                                spread.executable_qty,
                                false,
                                false,
                                opp.sell_price * Fixed8::from_f64(0.9995),
                                opp.sell_price * Fixed8::from_f64(1.0005),
                                now_ns,
                            );

                            self.orphaned_tracker.record_fill(spread.leg_a_buy.client_order_id, now_ns);
                            self.orphaned_tracker.record_fill(spread.leg_b_sell.client_order_id, now_ns);

                            self.total_spreads_executed += 1;
                            executed_intents.push(spread.leg_a_buy);
                            executed_intents.push(spread.leg_b_sell);
                        }
                    }
                }
            }
        }

        // Collect Candidate Orders for Directional / Microstructure Strategies (Dual-Cohort Showdown)
        let mut candidates: Vec<StrategyCandidate> = Vec::new();
        let macro_bias = self.active_macro.macro_bias;
        let books_snapshot: Vec<aeris_feed::L2OrderBook> = self.books.values().cloned().collect();

        // 4a. Strategy: Stoikov Micro-Divergence (Old Chap - Raw)
        for book in &books_snapshot {
            let key = (StrategyId::StoikovRaw, book.venue, book.symbol);
            if !self.paper_positions.contains_key(&key) {
                if let Some(intent) = self.raw_momentum_engine.evaluate_signal(book, macro_bias, now_ns) {
                    let pass_macro = match (intent.side, macro_bias) {
                        (Side::Sell, 1) => false,
                        (Side::Buy, 2) => false,
                        _ => true,
                    };

                    if pass_macro && self.risk_gateway.evaluate_order(&intent, &self.account, Some(intent.price)).is_ok() {
                        let (tp, sl) = match intent.side {
                            Side::Buy => (
                                intent.price * Fixed8::from_f64(1.00030),
                                intent.price * Fixed8::from_f64(0.99975),
                            ),
                            Side::Sell => (
                                intent.price * Fixed8::from_f64(0.99970),
                                intent.price * Fixed8::from_f64(1.00025),
                            ),
                        };
                        candidates.push(StrategyCandidate {
                            strategy: StrategyId::StoikovRaw,
                            intent,
                            take_profit_price: tp,
                            stop_loss_price: sl,
                        });
                    }
                }
            }
        }

        // 4b. Strategy: Stoikov Micro-Divergence (Calibrated Fleet - 15s Debounce, 24x Hurdle 0.12 bps, 1.5x Depth Skew, 3:1 R:R)
        for book in &books_snapshot {
            let key = (StrategyId::StoikovCalibrated, book.venue, book.symbol);
            let last_entry = self.last_entry_time_ns.get(&key).copied().unwrap_or(0);
            let is_debounced = now_ns.saturating_sub(last_entry) >= 15_000_000_000;

            if is_debounced && !self.paper_positions.contains_key(&key) {
                if let Some(intent) = self.calib_momentum_engine.evaluate_signal(book, macro_bias, now_ns) {
                    let pass_macro = match (intent.side, macro_bias) {
                        (Side::Sell, 1) => false,
                        (Side::Buy, 2) => false,
                        _ => true,
                    };

                    // Order flow depth confirmation: require 1.5x depth on intent side
                    let bid_depth: Fixed8 = book.bids().iter().take(5).fold(Fixed8::ZERO, |acc, b| acc + b.qty);
                    let ask_depth: Fixed8 = book.asks().iter().take(5).fold(Fixed8::ZERO, |acc, a| acc + a.qty);
                    let has_depth_confirm = match intent.side {
                        Side::Buy => bid_depth >= ask_depth * Fixed8::from_f64(1.5),
                        Side::Sell => ask_depth >= bid_depth * Fixed8::from_f64(1.5),
                    };

                    if pass_macro && has_depth_confirm && self.risk_gateway.evaluate_order(&intent, &self.account, Some(intent.price)).is_ok() {
                        let (tp, sl) = match intent.side {
                            Side::Buy => (
                                intent.price * Fixed8::from_f64(1.00150),
                                intent.price * Fixed8::from_f64(0.99950),
                            ),
                            Side::Sell => (
                                intent.price * Fixed8::from_f64(0.99850),
                                intent.price * Fixed8::from_f64(1.00050),
                            ),
                        };
                        candidates.push(StrategyCandidate {
                            strategy: StrategyId::StoikovCalibrated,
                            intent,
                            take_profit_price: tp,
                            stop_loss_price: sl,
                        });
                    }
                }
            }
        }

        // 5a. Strategy: Aggressive Breakout (Old Chap - Raw)
        for book in &books_snapshot {
            let key = (StrategyId::BreakoutRaw, book.venue, book.symbol);
            if !self.paper_positions.contains_key(&key) {
                let mids = self.recent_mids.get(&(book.venue, book.symbol)).cloned().unwrap_or_default();
                if let Some(intent) = self.raw_breakout_engine.evaluate(book, &mids, now_ns) {
                    let pass_macro = match (intent.side, macro_bias) {
                        (Side::Sell, 1) => false,
                        (Side::Buy, 2) => false,
                        _ => true,
                    };

                    if pass_macro && self.risk_gateway.evaluate_order(&intent, &self.account, Some(intent.price)).is_ok() {
                        let (tp, sl) = match intent.side {
                            Side::Buy => (
                                intent.price * Fixed8::from_f64(1.00050),
                                intent.price * Fixed8::from_f64(0.99970),
                            ),
                            Side::Sell => (
                                intent.price * Fixed8::from_f64(0.99950),
                                intent.price * Fixed8::from_f64(1.00030),
                            ),
                        };
                        candidates.push(StrategyCandidate {
                            strategy: StrategyId::BreakoutRaw,
                            intent,
                            take_profit_price: tp,
                            stop_loss_price: sl,
                        });
                    }
                }
            }
        }

        // 5b. Strategy: Aggressive Breakout (Calibrated Fleet - 15s Debounce, 5x Hurdle, 2.67:1 R:R)
        for book in &books_snapshot {
            let key = (StrategyId::BreakoutCalibrated, book.venue, book.symbol);
            let last_entry = self.last_entry_time_ns.get(&key).copied().unwrap_or(0);
            let is_debounced = now_ns.saturating_sub(last_entry) >= 15_000_000_000;

            if is_debounced && !self.paper_positions.contains_key(&key) {
                let mids = self.recent_mids.get(&(book.venue, book.symbol)).cloned().unwrap_or_default();
                if let Some(intent) = self.calib_breakout_engine.evaluate(book, &mids, now_ns) {
                    let pass_macro = match (intent.side, macro_bias) {
                        (Side::Sell, 1) => false,
                        (Side::Buy, 2) => false,
                        _ => true,
                    };

                    if pass_macro && self.risk_gateway.evaluate_order(&intent, &self.account, Some(intent.price)).is_ok() {
                        let (tp, sl) = match intent.side {
                            Side::Buy => (
                                intent.price * Fixed8::from_f64(1.00160),
                                intent.price * Fixed8::from_f64(0.99940),
                            ),
                            Side::Sell => (
                                intent.price * Fixed8::from_f64(0.99840),
                                intent.price * Fixed8::from_f64(1.00060),
                            ),
                        };
                        candidates.push(StrategyCandidate {
                            strategy: StrategyId::BreakoutCalibrated,
                            intent,
                            take_profit_price: tp,
                            stop_loss_price: sl,
                        });
                    }
                }
            }
        }

        // 6a. Strategy: Counter-Punch Micro-Wick (Old Chap - Raw)
        for book in &books_snapshot {
            let key = (StrategyId::CounterPunchRaw, book.venue, book.symbol);
            if !self.paper_positions.contains_key(&key) {
                let mids = self.recent_mids.get(&(book.venue, book.symbol)).cloned().unwrap_or_default();
                if mids.len() >= 3 {
                    let mean: Fixed8 = mids.iter().fold(Fixed8::ZERO, |acc, m| acc + *m) / Fixed8::from_f64(mids.len() as f64);
                    if let Some(intent) = self.raw_counter_punch_engine.evaluate(book, mean, now_ns) {
                        let pass_macro = match (intent.side, macro_bias) {
                            (Side::Sell, 1) => false,
                            (Side::Buy, 2) => false,
                            _ => true,
                        };

                        if pass_macro && self.risk_gateway.evaluate_order(&intent, &self.account, Some(intent.price)).is_ok() {
                            let (tp, sl) = match intent.side {
                                Side::Buy => (
                                    intent.price * Fixed8::from_f64(1.00040),
                                    intent.price * Fixed8::from_f64(0.99975),
                                ),
                                Side::Sell => (
                                    intent.price * Fixed8::from_f64(0.99960),
                                    intent.price * Fixed8::from_f64(1.00025),
                                ),
                            };
                            candidates.push(StrategyCandidate {
                                strategy: StrategyId::CounterPunchRaw,
                                intent,
                                take_profit_price: tp,
                                stop_loss_price: sl,
                            });
                        }
                    }
                }
            }
        }

        // 6b. Strategy: Counter-Punch Micro-Wick (Calibrated Fleet - 15s Debounce, 3.3x Hurdle, Depth Replenishment, 2.4:1 R:R)
        for book in &books_snapshot {
            let key = (StrategyId::CounterPunchCalibrated, book.venue, book.symbol);
            let last_entry = self.last_entry_time_ns.get(&key).copied().unwrap_or(0);
            let is_debounced = now_ns.saturating_sub(last_entry) >= 15_000_000_000;

            if is_debounced && !self.paper_positions.contains_key(&key) {
                let mids = self.recent_mids.get(&(book.venue, book.symbol)).cloned().unwrap_or_default();
                if mids.len() >= 3 {
                    let mean: Fixed8 = mids.iter().fold(Fixed8::ZERO, |acc, m| acc + *m) / Fixed8::from_f64(mids.len() as f64);
                    if let Some(intent) = self.calib_counter_punch_engine.evaluate(book, mean, now_ns) {
                        let pass_macro = match (intent.side, macro_bias) {
                            (Side::Sell, 1) => false,
                            (Side::Buy, 2) => false,
                            _ => true,
                        };

                        // Depth replenishment check: counter side volume must be >= 1.5x trend side volume
                        let bid_depth: Fixed8 = book.bids().iter().take(5).fold(Fixed8::ZERO, |acc, b| acc + b.qty);
                        let ask_depth: Fixed8 = book.asks().iter().take(5).fold(Fixed8::ZERO, |acc, a| acc + a.qty);
                        let has_replenishment = match intent.side {
                            Side::Buy => bid_depth >= ask_depth * Fixed8::from_f64(1.5),
                            Side::Sell => ask_depth >= bid_depth * Fixed8::from_f64(1.5),
                        };

                        if pass_macro && has_replenishment && self.risk_gateway.evaluate_order(&intent, &self.account, Some(intent.price)).is_ok() {
                            let (tp, sl) = match intent.side {
                                Side::Buy => (
                                    intent.price * Fixed8::from_f64(1.00120),
                                    intent.price * Fixed8::from_f64(0.99950),
                                ),
                                Side::Sell => (
                                    intent.price * Fixed8::from_f64(0.99880),
                                    intent.price * Fixed8::from_f64(1.00050),
                                ),
                            };
                            candidates.push(StrategyCandidate {
                                strategy: StrategyId::CounterPunchCalibrated,
                                intent,
                                take_profit_price: tp,
                                stop_loss_price: sl,
                            });
                        }
                    }
                }
            }
        }

        // =====================================================================
        // 7. INTERNAL CROSSING (DARK NETTING) EXECUTION ENGINE
        // =====================================================================
        let mut grouped: HashMap<(Venue, Symbol), (Vec<StrategyCandidate>, Vec<StrategyCandidate>)> = HashMap::new();
        for c in candidates {
            let entry = grouped.entry((c.intent.venue, c.intent.symbol)).or_insert_with(|| (Vec::new(), Vec::new()));
            match c.intent.side {
                Side::Buy => entry.0.push(c),
                Side::Sell => entry.1.push(c),
            }
        }

        for ((venue, sym), (mut buys, mut sells)) in grouped {
            let mid_px = self.books.get(&(venue, sym)).and_then(|b| b.mid_price());

            // Match opposing internal buy and sell orders at mid-price
            while !buys.is_empty() && !sells.is_empty() {
                let mut buy_cand = buys.pop().unwrap();
                let mut sell_cand = sells.pop().unwrap();

                let match_qty = buy_cand.intent.qty.min(sell_cand.intent.qty);
                let cross_px = mid_px.unwrap_or(buy_cand.intent.price);

                let notional = cross_px * match_qty;
                // Roundtrip taker fee saved (5 bps + 5 bps = 10 bps)
                let fees_saved = (notional * Fixed8::from_f64(10.0)) / Fixed8::from_f64(10000.0);
                self.total_fees_saved = self.total_fees_saved + fees_saved;
                self.total_internal_crosses += 1;

                let m_buy = self.strategy_metrics.entry(buy_cand.strategy).or_default();
                m_buy.fees_saved = m_buy.fees_saved + (fees_saved / Fixed8::from_f64(2.0));
                let m_sell = self.strategy_metrics.entry(sell_cand.strategy).or_default();
                m_sell.fees_saved = m_sell.fees_saved + (fees_saved / Fixed8::from_f64(2.0));

                println!(
                    "  [DARK POOL NETTING] 🔥 MATCHED {:.4} {:?} {} @ ${:.2}! [{}] (Buy) <-> [{}] (Sell) | Exchange Fees Saved: +${:.4}",
                    match_qty.to_f64(),
                    venue,
                    sym.as_str(),
                    cross_px.to_f64(),
                    buy_cand.strategy.name(),
                    sell_cand.strategy.name(),
                    fees_saved.to_f64()
                );

                // Internal zero-fee execution for Buy
                self.apply_strategy_fill(
                    buy_cand.strategy,
                    &buy_cand.intent,
                    cross_px,
                    match_qty,
                    false,
                    true, // is_internal_cross = true (ZERO FEE)
                    buy_cand.take_profit_price,
                    buy_cand.stop_loss_price,
                    now_ns,
                );
                executed_intents.push(buy_cand.intent);

                // Internal zero-fee execution for Sell
                self.apply_strategy_fill(
                    sell_cand.strategy,
                    &sell_cand.intent,
                    cross_px,
                    match_qty,
                    false,
                    true, // is_internal_cross = true (ZERO FEE)
                    sell_cand.take_profit_price,
                    sell_cand.stop_loss_price,
                    now_ns,
                );
                executed_intents.push(sell_cand.intent);

                // Handle residuals
                if buy_cand.intent.qty > match_qty {
                    buy_cand.intent.qty = buy_cand.intent.qty - match_qty;
                    buys.push(buy_cand);
                } else if sell_cand.intent.qty > match_qty {
                    sell_cand.intent.qty = sell_cand.intent.qty - match_qty;
                    sells.push(sell_cand);
                }
            }

            // Route uncrossed buy residuals to exchange
            for rem_buy in buys {
                let is_maker = rem_buy.strategy.is_calibrated();
                self.apply_strategy_fill(
                    rem_buy.strategy,
                    &rem_buy.intent,
                    rem_buy.intent.price,
                    rem_buy.intent.qty,
                    is_maker,
                    false,
                    rem_buy.take_profit_price,
                    rem_buy.stop_loss_price,
                    now_ns,
                );
                executed_intents.push(rem_buy.intent);
            }

            // Route uncrossed sell residuals to exchange
            for rem_sell in sells {
                let is_maker = rem_sell.strategy.is_calibrated();
                self.apply_strategy_fill(
                    rem_sell.strategy,
                    &rem_sell.intent,
                    rem_sell.intent.price,
                    rem_sell.intent.qty,
                    is_maker,
                    false,
                    rem_sell.take_profit_price,
                    rem_sell.stop_loss_price,
                    now_ns,
                );
                executed_intents.push(rem_sell.intent);
            }
        }

        executed_intents
    }

    pub fn print_status(&self) {
        let unrealized_pnl = self.compute_unrealized_pnl();
        let total_equity = self.account.total_equity + unrealized_pnl;
        let total_return_pct = if !self.initial_equity.is_zero() {
            ((total_equity - self.initial_equity) / self.initial_equity) * Fixed8::from_f64(100.0)
        } else {
            Fixed8::ZERO
        };
        let leverage = if !total_equity.is_zero() {
            self.account.total_deployed_notional / total_equity
        } else {
            Fixed8::ZERO
        };

        println!("================================================================================");
        println!(
            " AERIS LIVE PAPER ENGINE [Cycle #{}] | Equity: ${:.2} ({:+.3}%) | Deployed: ${:.2} ({:.2}x)",
            self.cycle_count,
            total_equity.to_f64(),
            total_return_pct.to_f64(),
            self.account.total_deployed_notional.to_f64(),
            leverage.to_f64()
        );
        println!(
            " Realized PnL: ${:+.4} | Unrealized PnL: ${:+.4} | Fees Paid: ${:.4} | §194S TDS: ${:.4}",
            self.realized_pnl.to_f64(),
            unrealized_pnl.to_f64(),
            self.total_fees_paid.to_f64(),
            self.total_tds_paid.to_f64()
        );
        println!(
            " Macro Bias: {} | Dark Crosses: {} | Fees Saved: +${:.4} | Fills: {}",
            match self.active_macro.macro_bias {
                1 => "SUPPLY_SQUEEZE (Bullish)",
                2 => "DISTRIBUTION (Bearish)",
                _ => "NEUTRAL / CHOP",
            },
            self.total_internal_crosses,
            self.total_fees_saved.to_f64(),
            self.total_fills_count
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

        if !self.paper_positions.is_empty() {
            println!("   --- ACTIVE PAPER INVENTORY ---");
            for ((strat, venue, sym), pos) in &self.paper_positions {
                println!(
                    "    * [{}] {:?} {} {:?}: {:.4} units @ ${:.2} (TP: ${:.2} | SL: ${:.2})",
                    strat.name(),
                    venue,
                    sym.as_str(),
                    pos.side,
                    pos.qty.to_f64(),
                    pos.avg_entry_price.to_f64(),
                    pos.take_profit_price.to_f64(),
                    pos.stop_loss_price.to_f64()
                );
            }
        }

        // SHOWDOWN LEADERBOARD
        let strats = [
            StrategyId::BreakoutCalibrated,
            StrategyId::StoikovCalibrated,
            StrategyId::CounterPunchCalibrated,
            StrategyId::BreakoutRaw,
            StrategyId::StoikovRaw,
            StrategyId::CounterPunchRaw,
            StrategyId::CrossVenueArb,
        ];

        let mut leaderboard_data = Vec::new();
        for &st in &strats {
            let m = self.strategy_metrics.get(&st).cloned().unwrap_or_default();

            let mut u_pnl = Fixed8::ZERO;
            for ((s, venue, sym), pos) in &self.paper_positions {
                if *s == st {
                    if let Some(book) = self.books.get(&(*venue, *sym)) {
                        let exit_px = match pos.side {
                            Side::Buy => book.best_bid().map(|b| b.price),
                            Side::Sell => book.best_ask().map(|a| a.price),
                        };
                        if let Some(px) = exit_px {
                            let gross = match pos.side {
                                Side::Buy => (px - pos.avg_entry_price) * pos.qty,
                                Side::Sell => (pos.avg_entry_price - px) * pos.qty,
                            };
                            let notional = px * pos.qty;
                            let est_fee = self.friction.compute_fee(notional, false);
                            let exit_side = match pos.side {
                                Side::Buy => Side::Sell,
                                Side::Sell => Side::Buy,
                            };
                            let est_tds = self.friction.compute_tds(notional, exit_side);
                            u_pnl = u_pnl + gross - est_fee - est_tds;
                        }
                    }
                }
            }

            let net_alpha = m.net_realized_pnl + u_pnl;
            let net_win_rate = if m.closed_trades > 0 {
                (m.winning_trades as f64 / m.closed_trades as f64) * 100.0
            } else {
                0.0
            };
            let gross_win_rate = if m.closed_trades > 0 {
                (m.gross_winning_trades as f64 / m.closed_trades as f64) * 100.0
            } else {
                0.0
            };

            leaderboard_data.push((st, m, u_pnl, net_alpha, net_win_rate, gross_win_rate));
        }

        leaderboard_data.sort_by(|a, b| b.3.cmp(&a.3));

        println!("-----------------------------------------------------------------------------------------------------------------------------");
        println!("                         AERIS MULTI-STRATEGY SHOWDOWN LEADERBOARD (UNBIASED DOUBLE-ENTRY MTM)                               ");
        println!("-----------------------------------------------------------------------------------------------------------------------------");
        println!(" Rank | Strategy           | Cohort     | Trades | Gross W% | Net W%  | Gross Realized | Closed Fees | Net Realized | Open MTM  | Total Alpha ");
        println!("------+--------------------+------------+--------+----------+---------+----------------+-------------+--------------+-----------+-------------");

        for (idx, (st, m, u_pnl, net_alpha, net_win_rate, gross_win_rate)) in leaderboard_data.iter().enumerate() {
            let rank_str = format!("#{}", idx + 1);

            println!(
                " {:4} | {:18} | {:10} | {:6} | {:7.1}% | {:6.1}% | ${:+13.2} | -${:10.2} | ${:+11.2} | ${:+8.2} | ${:+.4} ",
                rank_str,
                st.name(),
                st.cohort(),
                m.closed_trades,
                gross_win_rate,
                net_win_rate,
                m.gross_realized_pnl.to_f64(),
                m.closed_fees_paid.to_f64(),
                m.net_realized_pnl.to_f64(),
                u_pnl.to_f64(),
                net_alpha.to_f64()
            );
        }

        // COHORT AGGREGATE SUMMARY TABLE
        println!("-----------------------------------------------------------------------------------------------------------------------------");
        println!("                         COHORT HEAD-TO-HEAD SHOWDOWN: OLD CHAPS vs CALIBRATED FLEET (RECONCILED)                            ");
        println!("-----------------------------------------------------------------------------------------------------------------------------");
        println!(" Cohort     | Trades | Gross W% | Net W%  | Gross Realized | Closed Fees | Net Realized | Open MTM  | Total Alpha | Fee/Alpha Ratio   ");
        println!("------------+--------+----------+---------+----------------+-------------+--------------+-----------+-------------+-------------------");

        let cohorts = ["OLD CHAPS", "CALIBRATED", "ARBITRAGE"];
        for cohort_name in &cohorts {
            let mut c_trades: u64 = 0;
            let mut c_gross_wins: u64 = 0;
            let mut c_net_wins: u64 = 0;
            let mut c_gross_pnl = Fixed8::ZERO;
            let mut c_closed_fees = Fixed8::ZERO;
            let mut c_net_realized = Fixed8::ZERO;
            let mut c_u_pnl = Fixed8::ZERO;
            let mut c_net_alpha = Fixed8::ZERO;

            for (st, m, u_pnl, net_alpha, _, _) in &leaderboard_data {
                if st.cohort() == *cohort_name {
                    c_trades += m.closed_trades;
                    c_gross_wins += m.gross_winning_trades;
                    c_net_wins += m.winning_trades;
                    c_gross_pnl = c_gross_pnl + m.gross_realized_pnl;
                    c_closed_fees = c_closed_fees + m.closed_fees_paid;
                    c_net_realized = c_net_realized + m.net_realized_pnl;
                    c_u_pnl = c_u_pnl + *u_pnl;
                    c_net_alpha = c_net_alpha + *net_alpha;
                }
            }

            let c_gross_wr = if c_trades > 0 { (c_gross_wins as f64 / c_trades as f64) * 100.0 } else { 0.0 };
            let c_net_wr = if c_trades > 0 { (c_net_wins as f64 / c_trades as f64) * 100.0 } else { 0.0 };
            let fee_ratio_str = if c_gross_pnl.to_f64().abs() > 0.001 {
                let ratio = c_closed_fees.to_f64() / c_gross_pnl.to_f64().abs();
                if ratio > 1.0 {
                    format!("{:6.1}x (BURN)", ratio)
                } else {
                    format!("{:6.2}x (EDGE)", ratio)
                }
            } else if c_closed_fees.is_positive() {
                "  INFx (BURN)".to_string()
            } else {
                "  0.00x (FLAT)".to_string()
            };

            println!(
                " {:10} | {:6} | {:7.1}% | {:6.1}% | ${:+13.2} | -${:10.2} | ${:+11.2} | ${:+8.2} | ${:+10.2} | {:17}",
                cohort_name,
                c_trades,
                c_gross_wr,
                c_net_wr,
                c_gross_pnl.to_f64(),
                c_closed_fees.to_f64(),
                c_net_realized.to_f64(),
                c_u_pnl.to_f64(),
                c_net_alpha.to_f64(),
                fee_ratio_str
            );
        }

        let best = &leaderboard_data[0];
        println!("------------------------------------------------------------------------------------------------------");
        println!(
            " 🏆 STRONGEST STRATEGY: {} [{}] (Net Alpha: ${:+.4} | Net Win%: {:.1}% | Saved Fees: +${:.4})",
            best.0.name(),
            best.0.cohort(),
            best.3.to_f64(),
            best.4,
            best.1.fees_saved.to_f64()
        );
        println!("======================================================================================================\n");
    }
}

#[tokio::main]
async fn main() {
    println!("\n================================================================================");
    println!("             AERIS PRODUCTION ENGINE NODE (ARMORED RUST RUNTIME)                ");
    println!("================================================================================\n");

    let args: Vec<String> = std::env::args().collect();
    let is_live = args.iter().any(|a| a == "--live");
    let is_onshore = args.iter().any(|a| a == "--onshore");
    let duration_mins: Option<u64> = args
        .iter()
        .position(|a| a == "--duration-mins")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    let max_ticks: Option<u64> = args
        .iter()
        .position(|a| a == "--ticks")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .or_else(|| duration_mins.map(|m| m * 120));

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\n[SHUTDOWN] Received Ctrl+C. Initiating graceful shutdown of Aeris Node...");
        r.store(false, Ordering::SeqCst);
    });

    let mut node = AerisEngineNode::new(
        Fixed8::from_f64(100_000.0),
        is_onshore,
        Some(PathBuf::from("D:/Pushp - Do not open without permission/AERIS/data/aeris_shm.dat")),
    );

    let gd_target = "127.0.0.1:9876";
    let gd_running = running.clone();
    tokio::spawn(async move {
        let socket = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(_) => return,
        };
        let dest = gd_target.to_string();
        let mut seq: u64 = 0;
        let auth_token = aeris_gd::AUTH_MAGIC_TOKEN;

        while gd_running.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(200)).await;
            seq += 1;
            let packet = aeris_gd::HeartbeatPacket {
                auth_token,
                seq,
                timestamp_ns: current_time_ns(),
                active_positions_count: 0,
                is_disarm: false,
            };
            if let Ok(bytes) = serde_json::to_vec(&packet) {
                let _ = socket.send_to(&bytes, &dest).await;
            }
        }

        let disarm = aeris_gd::HeartbeatPacket {
            auth_token,
            seq: seq + 1,
            timestamp_ns: current_time_ns(),
            active_positions_count: 0,
            is_disarm: true,
        };
        if let Ok(bytes) = serde_json::to_vec(&disarm) {
            let _ = socket.send_to(&bytes, &dest).await;
        }
    });

    if is_live {
        println!("[INFO] Initialized node with $100,000.00 equity in LIVE STREAMING mode.");
        let (tx, mut rx) = mpsc::channel::<BookUpdate>(1024);

        let symbols = ["BTCUSDT", "ETHUSDT"];
        for &sym in &symbols {
            let tx_binance = tx.clone();
            tokio::spawn(async move {
                println!("[FEED] Spawning Binance WebSocket feed handler for {}...", sym);
                if let Err(e) = connect_binance_depth(sym, tx_binance).await {
                    eprintln!("[WARN] Binance WS {} terminated: {:?}", sym, e);
                }
            });

            let tx_bybit = tx.clone();
            tokio::spawn(async move {
                println!("[FEED] Spawning Bybit WebSocket feed handler for {}...", sym);
                if let Err(e) = connect_bybit_depth(sym, tx_bybit).await {
                    eprintln!("[WARN] Bybit WS {} terminated: {:?}", sym, e);
                }
            });
        }

        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut ticks: u64 = 0;

        while running.load(Ordering::SeqCst) {
            tokio::select! {
                biased;
                Some(update) = rx.recv() => {
                    node.on_book_update(&update);
                }
                _ = interval.tick() => {
                    node.execute_cycle();
                    ticks += 1;
                    if ticks % 10 == 0 || (max_ticks.is_some() && ticks >= max_ticks.unwrap()) {
                        node.print_status();
                    }
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

        let mut tick_counter: u64 = 0;
        let limit = max_ticks.unwrap_or(20);

        while running.load(Ordering::SeqCst) && tick_counter < limit {
            tick_counter += 1;

            let phase = tick_counter as f64;
            let btc_base = 60_000.0 + (phase * 1.5).sin() * 8.0 + (if phase > 10.0 { 6.0 } else { 0.0 });
            let btc_bid = btc_base;
            let btc_ask = btc_base + 1.0;

            let bid_q = if phase % 4.0 == 1.0 { 15.0 } else { 2.0 };
            let ask_q = if phase % 4.0 == 3.0 { 15.0 } else { 2.0 };

            let btc_update = BookUpdate {
                venue: Venue::Binance,
                symbol: btc_sym,
                timestamp_ns: current_time_ns(),
                bids: vec![
                    BookLevel {
                        price: Fixed8::from_f64(btc_bid),
                        qty: Fixed8::from_f64(bid_q),
                    },
                    BookLevel {
                        price: Fixed8::from_f64(btc_bid - 5.0),
                        qty: Fixed8::from_f64(20.0),
                    },
                ],
                asks: vec![
                    BookLevel {
                        price: Fixed8::from_f64(btc_ask),
                        qty: Fixed8::from_f64(ask_q),
                    },
                    BookLevel {
                        price: Fixed8::from_f64(btc_ask + 5.0),
                        qty: Fixed8::from_f64(10.0),
                    },
                ],
                is_snapshot: true,
            };
            node.on_book_update(&btc_update);

            let bybit_btc_update = BookUpdate {
                venue: Venue::Bybit,
                symbol: btc_sym,
                timestamp_ns: current_time_ns(),
                bids: vec![BookLevel {
                    price: Fixed8::from_f64(btc_bid - 0.5),
                    qty: Fixed8::from_f64(10.0),
                }],
                asks: vec![BookLevel {
                    price: Fixed8::from_f64(btc_ask + 0.5),
                    qty: Fixed8::from_f64(10.0),
                }],
                is_snapshot: true,
            };
            node.on_book_update(&bybit_btc_update);

            node.execute_cycle();
            node.print_status();

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    println!("\n================================================================================");
    println!("             FINAL MULTI-STRATEGY SOAK BENCHMARK REPORT                         ");
    println!("================================================================================");
    node.print_status();
    println!("[INFO] Node execution completed cleanly.");
}
