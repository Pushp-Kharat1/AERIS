use aeris_core::{
    Fixed8, OrderIntent, OrderType, Side, TimeInForce, TimestampNs,
};
use aeris_feed::L2OrderBook;

pub struct CounterPunchEngine {
    pub stretch_hurdle_bps: Fixed8,
    pub default_qty: Fixed8,
    pub stop_loss_bps: Fixed8,
    pub take_profit_bps: Fixed8,
    next_order_id: u64,
}

impl CounterPunchEngine {
    pub fn new(stretch_hurdle_bps: Fixed8, default_qty: Fixed8) -> Self {
        Self {
            stretch_hurdle_bps,
            default_qty,
            stop_loss_bps: Fixed8::from_f64(0.8),
            take_profit_bps: Fixed8::from_f64(1.2),
            next_order_id: 4_000_000,
        }
    }

    pub fn evaluate(
        &mut self,
        book: &L2OrderBook,
        rolling_mean: Fixed8,
        now_ns: TimestampNs,
    ) -> Option<OrderIntent> {
        let mid = book.mid_price()?;
        if rolling_mean.is_zero() {
            return None;
        }

        let stretch_bps = ((mid - rolling_mean) / rolling_mean) * Fixed8::from_f64(10_000.0);
        let div_bps = book.micro_price_divergence_bps().unwrap_or(Fixed8::ZERO);

        let order_id = self.next_order_id;

        // Oversold dip: price stretched downwards, but micro-price shows buyer absorption (div > 0)
        if stretch_bps <= -self.stretch_hurdle_bps && div_bps > Fixed8::ZERO {
            self.next_order_id += 1;
            let price = book.best_ask()?.price;
            return Some(OrderIntent {
                client_order_id: order_id,
                venue: book.venue,
                symbol: book.symbol,
                side: Side::Buy,
                order_type: OrderType::ImmediateOrCancel,
                time_in_force: TimeInForce::ImmediateOrCancel,
                price,
                qty: self.default_qty,
                created_at_ns: now_ns,
                parent_leg_id: None,
            });
        }

        // Overbought spike: price stretched upwards, but micro-price shows seller absorption (div < 0)
        if stretch_bps >= self.stretch_hurdle_bps && div_bps < Fixed8::ZERO {
            self.next_order_id += 1;
            let price = book.best_bid()?.price;
            return Some(OrderIntent {
                client_order_id: order_id,
                venue: book.venue,
                symbol: book.symbol,
                side: Side::Sell,
                order_type: OrderType::ImmediateOrCancel,
                time_in_force: TimeInForce::ImmediateOrCancel,
                price,
                qty: self.default_qty,
                created_at_ns: now_ns,
                parent_leg_id: None,
            });
        }

        None
    }
}
