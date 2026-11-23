use aeris_core::{Fixed8, Side, TimestampNs};

#[derive(Clone, Debug)]
pub struct RestingLimitOrder {
    pub client_order_id: u64,
    pub side: Side,
    pub price: Fixed8,
    pub qty: Fixed8,
    pub remaining_qty: Fixed8,
    pub queue_ahead: Fixed8,
    pub placed_time_ns: TimestampNs,
    pub arrival_time_ns: TimestampNs,
    pub is_active_in_book: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SimulatedOrderQueue {
    pub resting_orders: Vec<RestingLimitOrder>,
}

impl SimulatedOrderQueue {
    pub fn new() -> Self {
        Self {
            resting_orders: Vec::new(),
        }
    }

    /// Place a new resting limit order
    pub fn place_order(
        &mut self,
        client_order_id: u64,
        side: Side,
        price: Fixed8,
        qty: Fixed8,
        latency_ns: u64,
        now_ns: TimestampNs,
    ) {
        self.resting_orders.push(RestingLimitOrder {
            client_order_id,
            side,
            price,
            qty,
            remaining_qty: qty,
            queue_ahead: Fixed8::ZERO,
            placed_time_ns: now_ns,
            arrival_time_ns: now_ns + latency_ns,
            is_active_in_book: false,
        });
    }

    /// Update arrival state based on current timestamp
    pub fn activate_arrived_orders(&mut self, now_ns: TimestampNs, book: &aeris_feed::L2OrderBook) {
        for order in &mut self.resting_orders {
            if !order.is_active_in_book && now_ns >= order.arrival_time_ns {
                order.is_active_in_book = true;
                // Compute initial queue depth ahead of our order
                order.queue_ahead = match order.side {
                    Side::Buy => book
                        .bids()
                        .iter()
                        .filter(|b| b.price >= order.price)
                        .fold(Fixed8::ZERO, |acc, b| acc + b.qty),
                    Side::Sell => book
                        .asks()
                        .iter()
                        .filter(|a| a.price <= order.price)
                        .fold(Fixed8::ZERO, |acc, a| acc + a.qty),
                };
            }
        }
    }

    /// Process a trade print against active resting orders in the queue
    /// Returns vector of (client_order_id, side, filled_qty, fill_price)
    pub fn on_trade_print(
        &mut self,
        trade_price: Fixed8,
        trade_qty: Fixed8,
        now_ns: TimestampNs,
    ) -> Vec<(u64, Side, Fixed8, Fixed8)> {
        let mut fills = Vec::new();

        self.resting_orders.retain_mut(|order| {
            if !order.is_active_in_book || now_ns < order.arrival_time_ns {
                return true; // Still in flight
            }

            let matches = match order.side {
                Side::Buy => trade_price <= order.price,
                Side::Sell => trade_price >= order.price,
            };

            if matches {
                if order.queue_ahead > trade_qty {
                    order.queue_ahead = order.queue_ahead - trade_qty;
                    true // Order still waiting in queue
                } else {
                    let volume_surplus = trade_qty - order.queue_ahead;
                    let fill_qty = if volume_surplus >= order.remaining_qty {
                        order.remaining_qty
                    } else {
                        volume_surplus
                    };

                    order.remaining_qty = order.remaining_qty - fill_qty;
                    order.queue_ahead = Fixed8::ZERO;

                    fills.push((order.client_order_id, order.side, fill_qty, order.price));

                    // Keep in queue only if partially filled
                    order.remaining_qty.is_positive()
                }
            } else {
                true
            }
        });

        fills
    }

    /// Cancel order
    pub fn cancel_order(&mut self, id: u64) {
        self.resting_orders.retain(|o| o.client_order_id != id);
    }
}
