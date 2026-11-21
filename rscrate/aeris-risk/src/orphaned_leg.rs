use aeris_core::{Fixed8, OrderIntent, OrderType, TimeInForce, TimestampNs};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct PendingSpread {
    pub spread_id: u64,
    pub leg_a_intent: OrderIntent,
    pub leg_b_intent: OrderIntent,
    pub leg_a_filled: bool,
    pub leg_a_fill_time_ns: Option<TimestampNs>,
    pub leg_b_filled: bool,
}

pub struct OrphanedLegTracker {
    timeout_ns: u64,
    pending_spreads: HashMap<u64, PendingSpread>,
}

impl OrphanedLegTracker {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_ns: timeout_ms * 1_000_000,
            pending_spreads: HashMap::new(),
        }
    }

    pub fn register_spread(&mut self, spread_id: u64, leg_a: OrderIntent, leg_b: OrderIntent) {
        self.pending_spreads.insert(
            spread_id,
            PendingSpread {
                spread_id,
                leg_a_intent: leg_a,
                leg_b_intent: leg_b,
                leg_a_filled: false,
                leg_a_fill_time_ns: None,
                leg_b_filled: false,
            },
        );
    }

    pub fn record_fill(&mut self, client_order_id: u64, timestamp_ns: TimestampNs) {
        for spread in self.pending_spreads.values_mut() {
            if spread.leg_a_intent.client_order_id == client_order_id {
                spread.leg_a_filled = true;
                spread.leg_a_fill_time_ns = Some(timestamp_ns);
            } else if spread.leg_b_intent.client_order_id == client_order_id {
                spread.leg_b_filled = true;
            }
        }
    }

    pub fn check_timeouts(&mut self, current_time_ns: TimestampNs) -> Vec<OrderIntent> {
        let mut emergency_unwinds = Vec::new();
        let mut completed_or_timed_out = Vec::new();

        for (spread_id, spread) in self.pending_spreads.iter() {
            if spread.leg_a_filled && spread.leg_b_filled {
                completed_or_timed_out.push(*spread_id);
            } else if spread.leg_a_filled && !spread.leg_b_filled {
                if let Some(fill_time) = spread.leg_a_fill_time_ns {
                    if current_time_ns.saturating_sub(fill_time) >= self.timeout_ns {
                        let unwind_intent = OrderIntent {
                            client_order_id: spread.leg_a_intent.client_order_id + 10_000_000,
                            venue: spread.leg_a_intent.venue,
                            symbol: spread.leg_a_intent.symbol,
                            side: spread.leg_a_intent.side.opposite(),
                            order_type: OrderType::ImmediateOrCancel,
                            time_in_force: TimeInForce::ImmediateOrCancel,
                            price: Fixed8::ZERO,
                            qty: spread.leg_a_intent.qty,
                            created_at_ns: current_time_ns,
                            parent_leg_id: Some(spread.leg_a_intent.client_order_id),
                        };
                        emergency_unwinds.push(unwind_intent);
                        completed_or_timed_out.push(*spread_id);
                    }
                }
            }
        }

        for id in completed_or_timed_out {
            self.pending_spreads.remove(&id);
        }

        emergency_unwinds
    }
}
