use std::collections::HashMap;
use aeris_core::{Fixed8, OrderIntent, OrderType, Side, Symbol, TimeInForce, TimestampNs, Venue};
use aeris_execution::FrictionModel;
use aeris_feed::L2OrderBook;

use crate::config::BacktestConfig;
use crate::events::{MarketEvent, TradePrint};
use crate::queue::SimulatedOrderQueue;

#[derive(Clone, Debug)]
pub struct SimulatedPosition {
    pub symbol: Symbol,
    pub side: Side,
    pub qty: Fixed8,
    pub avg_entry_price: Fixed8,
    pub entry_time_ns: TimestampNs,
    pub is_maker_entry: bool,
    pub entry_fee_paid: Fixed8,
    pub entry_tds_paid: Fixed8,
}

#[derive(Clone, Debug)]
pub struct ClosedTradeRecord {
    pub symbol: Symbol,
    pub side: Side,
    pub qty: Fixed8,
    pub entry_price: Fixed8,
    pub exit_price: Fixed8,
    pub gross_pnl: Fixed8,
    pub total_fees: Fixed8,
    pub total_tds: Fixed8,
    pub net_pnl: Fixed8,
    pub entry_time_ns: TimestampNs,
    pub exit_time_ns: TimestampNs,
    pub holding_duration_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct BacktestReport {
    pub total_trades: u64,
    pub gross_winning_trades: u64,
    pub net_winning_trades: u64,
    pub gross_win_rate_pct: f64,
    pub net_win_rate_pct: f64,
    pub gross_realized_pnl: Fixed8,
    pub total_fees_paid: Fixed8,
    pub total_tds_paid: Fixed8,
    pub net_realized_pnl: Fixed8,
    pub fee_to_alpha_burn_ratio: f64,
    pub profit_factor: f64,
    pub max_drawdown_usd: Fixed8,
    pub max_drawdown_pct: f64,
    pub avg_holding_duration_ms: u64,
    pub final_equity: Fixed8,
}

pub struct FeeAndFillSimulator {
    pub config: BacktestConfig,
    pub book: L2OrderBook,
    pub queue: SimulatedOrderQueue,
    pub friction: FrictionModel,
    pub current_equity: Fixed8,
    pub peak_equity: Fixed8,
    pub max_drawdown_usd: Fixed8,
    pub positions: HashMap<Symbol, SimulatedPosition>,
    pub closed_trades: Vec<ClosedTradeRecord>,
    pub in_flight_takers: Vec<(OrderIntent, TimestampNs)>,
}

impl FeeAndFillSimulator {
    pub fn new(venue: Venue, symbol: Symbol, config: BacktestConfig) -> Self {
        let friction = if config.is_onshore {
            FrictionModel::default()
        } else {
            FrictionModel::offshore_linear()
        };
        let initial = config.initial_equity;

        Self {
            config,
            book: L2OrderBook::new(venue, symbol),
            queue: SimulatedOrderQueue::new(),
            friction,
            current_equity: initial,
            peak_equity: initial,
            max_drawdown_usd: Fixed8::ZERO,
            positions: HashMap::new(),
            closed_trades: Vec::new(),
            in_flight_takers: Vec::new(),
        }
    }

    /// Submit an order intent from a quantitative strategy
    pub fn submit_order(&mut self, intent: OrderIntent, now_ns: TimestampNs) {
        let latency_ns = self.config.network_latency_ms * 1_000_000;

        match (intent.order_type, intent.time_in_force) {
            (OrderType::ImmediateOrCancel, _)
            | (OrderType::Market, _)
            | (_, TimeInForce::ImmediateOrCancel) => {
                // Taker order in flight
                let arrival_time_ns = now_ns + latency_ns;
                self.in_flight_takers.push((intent, arrival_time_ns));
            }
            _ => {
                // Maker resting limit order joins queue after latency
                self.queue.place_order(
                    intent.client_order_id,
                    intent.side,
                    intent.price,
                    intent.qty,
                    latency_ns,
                    now_ns,
                );
            }
        }
    }

    /// Ingest a market event (Depth update, trade print, or liquidation)
    pub fn process_event(&mut self, event: MarketEvent, now_ns: TimestampNs) {
        match event {
            MarketEvent::Depth(update) => {
                self.book.apply_update(&update);
                self.queue.activate_arrived_orders(now_ns, &self.book);
                self.process_arrived_takers(now_ns);
            }
            MarketEvent::Trade(trade) => {
                self.queue.activate_arrived_orders(now_ns, &self.book);
                self.process_trade_against_queue(&trade, now_ns);
                self.process_arrived_takers(now_ns);
            }
            MarketEvent::Liquidation(liq) => {
                let trade = TradePrint {
                    price: liq.price,
                    qty: liq.qty,
                    is_buyer_maker: liq.side == Side::Sell,
                    timestamp_ns: liq.timestamp_ns,
                };
                self.queue.activate_arrived_orders(now_ns, &self.book);
                self.process_trade_against_queue(&trade, now_ns);
                self.process_arrived_takers(now_ns);
            }
        }
    }

    fn process_trade_against_queue(&mut self, trade: &TradePrint, now_ns: TimestampNs) {
        let queue_fills = self.queue.on_trade_print(trade.price, trade.qty, now_ns);

        for (_id, qty, price) in queue_fills {
            // Determine side based on which side of the book was filled
            let is_buy_maker = trade.is_buyer_maker; // If true, trade was aggressive sell into our resting bid
            let side = if is_buy_maker { Side::Buy } else { Side::Sell };

            // Apply adverse selection penalty if market moved aggressively through our price
            let effective_price = if side == Side::Buy {
                // If we bought resting, aggressive selling pushes mark down
                price - (price * self.config.adverse_selection_bps / Fixed8::from_int(10000))
            } else {
                price + (price * self.config.adverse_selection_bps / Fixed8::from_int(10000))
            };

            self.execute_fill(side, effective_price, qty, true, now_ns);
        }
    }

    fn process_arrived_takers(&mut self, now_ns: TimestampNs) {
        let mut ready = Vec::new();
        self.in_flight_takers.retain(|(intent, arrival)| {
            if now_ns >= *arrival {
                ready.push(intent.clone());
                false
            } else {
                true
            }
        });

        for intent in ready {
            // Walk L2 book levels to compute exact VWAP fill price
            if let Some((vwap_price, fill_qty)) = self.walk_book(intent.side, intent.qty, intent.price) {
                if fill_qty.is_positive() {
                    self.execute_fill(intent.side, vwap_price, fill_qty, false, now_ns);
                }
            }
        }
    }

    /// Walk L2 order book levels to calculate real fill price with slippage
    fn walk_book(&self, side: Side, requested_qty: Fixed8, price_limit: Fixed8) -> Option<(Fixed8, Fixed8)> {
        let mut remaining = requested_qty;
        let mut total_notional = Fixed8::ZERO;

        match side {
            Side::Buy => {
                for ask in self.book.asks() {
                    if ask.price > price_limit && price_limit.is_positive() {
                        break;
                    }
                    let fill_at_level = if ask.qty >= remaining { remaining } else { ask.qty };
                    total_notional = total_notional + (ask.price * fill_at_level);
                    remaining = remaining - fill_at_level;
                    if !remaining.is_positive() {
                        break;
                    }
                }
            }
            Side::Sell => {
                for bid in self.book.bids() {
                    if bid.price < price_limit && price_limit.is_positive() {
                        break;
                    }
                    let fill_at_level = if bid.qty >= remaining { remaining } else { bid.qty };
                    total_notional = total_notional + (bid.price * fill_at_level);
                    remaining = remaining - fill_at_level;
                    if !remaining.is_positive() {
                        break;
                    }
                }
            }
        }

        let executed_qty = requested_qty - remaining;
        if executed_qty.is_positive() {
            let vwap = total_notional / executed_qty;
            Some((vwap, executed_qty))
        } else {
            None
        }
    }

    fn execute_fill(&mut self, side: Side, price: Fixed8, qty: Fixed8, is_maker: bool, now_ns: TimestampNs) {
        let notional = price * qty;
        let fee_rate = if is_maker { self.config.maker_fee_bps } else { self.config.taker_fee_bps };
        let fee = notional * fee_rate / Fixed8::from_int(10000);
        let tds = self.friction.compute_tds(notional, side);

        let sym = self.book.symbol;

        if let Some(pos) = self.positions.get(&sym).cloned() {
            if pos.side != side {
                // Closing position
                let close_qty = if pos.qty >= qty { qty } else { pos.qty };

                let gross_pnl = match pos.side {
                    Side::Buy => (price - pos.avg_entry_price) * close_qty,
                    Side::Sell => (pos.avg_entry_price - price) * close_qty,
                };

                let total_fees = pos.entry_fee_paid + fee;
                let total_tds = pos.entry_tds_paid + tds;
                let net_pnl = gross_pnl - total_fees - total_tds;

                self.current_equity = self.current_equity + net_pnl;
                if self.current_equity > self.peak_equity {
                    self.peak_equity = self.current_equity;
                }
                let dd = self.peak_equity - self.current_equity;
                if dd > self.max_drawdown_usd {
                    self.max_drawdown_usd = dd;
                }

                let duration_ms = (now_ns.saturating_sub(pos.entry_time_ns)) / 1_000_000;

                self.closed_trades.push(ClosedTradeRecord {
                    symbol: sym,
                    side: pos.side,
                    qty: close_qty,
                    entry_price: pos.avg_entry_price,
                    exit_price: price,
                    gross_pnl,
                    total_fees,
                    total_tds,
                    net_pnl,
                    entry_time_ns: pos.entry_time_ns,
                    exit_time_ns: now_ns,
                    holding_duration_ms: duration_ms,
                });

                if pos.qty > close_qty {
                    let rem_qty = pos.qty - close_qty;
                    self.positions.insert(
                        sym,
                        SimulatedPosition {
                            qty: rem_qty,
                            ..pos
                        },
                    );
                } else {
                    self.positions.remove(&sym);
                }
                return;
            }
        }

        // Opening new position
        self.positions.insert(
            sym,
            SimulatedPosition {
                symbol: sym,
                side,
                qty,
                avg_entry_price: price,
                entry_time_ns: now_ns,
                is_maker_entry: is_maker,
                entry_fee_paid: fee,
                entry_tds_paid: tds,
            },
        );
    }

    /// Compile comprehensive performance analytics
    pub fn generate_report(&self) -> BacktestReport {
        let total_trades = self.closed_trades.len() as u64;
        let mut gross_winning = 0;
        let mut net_winning = 0;
        let mut total_gross_pnl = Fixed8::ZERO;
        let mut total_fees = Fixed8::ZERO;
        let mut total_tds = Fixed8::ZERO;
        let mut total_net_pnl = Fixed8::ZERO;
        let mut gross_wins_sum = Fixed8::ZERO;
        let mut gross_losses_sum = Fixed8::ZERO;
        let mut total_duration = 0;

        for t in &self.closed_trades {
            if t.gross_pnl.is_positive() {
                gross_winning += 1;
                gross_wins_sum = gross_wins_sum + t.gross_pnl;
            } else {
                gross_losses_sum = gross_losses_sum + t.gross_pnl.abs();
            }

            if t.net_pnl.is_positive() {
                net_winning += 1;
            }

            total_gross_pnl = total_gross_pnl + t.gross_pnl;
            total_fees = total_fees + t.total_fees;
            total_tds = total_tds + t.total_tds;
            total_net_pnl = total_net_pnl + t.net_pnl;
            total_duration += t.holding_duration_ms;
        }

        let gross_win_rate_pct = if total_trades > 0 {
            (gross_winning as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };

        let net_win_rate_pct = if total_trades > 0 {
            (net_winning as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };

        let profit_factor = if gross_losses_sum.is_positive() {
            gross_wins_sum.to_f64() / gross_losses_sum.to_f64()
        } else if gross_wins_sum.is_positive() {
            999.0
        } else {
            0.0
        };

        let fee_to_alpha_burn_ratio = if total_gross_pnl.is_positive() {
            total_fees.to_f64() / total_gross_pnl.to_f64()
        } else {
            999.0
        };

        let max_drawdown_pct = if self.peak_equity.is_positive() {
            (self.max_drawdown_usd.to_f64() / self.peak_equity.to_f64()) * 100.0
        } else {
            0.0
        };

        let avg_holding_duration_ms = if total_trades > 0 {
            total_duration / total_trades
        } else {
            0
        };

        BacktestReport {
            total_trades,
            gross_winning_trades: gross_winning,
            net_winning_trades: net_winning,
            gross_win_rate_pct,
            net_win_rate_pct,
            gross_realized_pnl: total_gross_pnl,
            total_fees_paid: total_fees,
            total_tds_paid: total_tds,
            net_realized_pnl: total_net_pnl,
            fee_to_alpha_burn_ratio,
            profit_factor,
            max_drawdown_usd: self.max_drawdown_usd,
            max_drawdown_pct,
            avg_holding_duration_ms,
            final_equity: self.current_equity,
        }
    }
}
