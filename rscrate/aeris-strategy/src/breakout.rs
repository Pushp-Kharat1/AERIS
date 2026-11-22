use aeris_core::{
    Fixed8, OrderIntent, OrderType, Side, TimeInForce, TimestampNs,
};
use aeris_feed::L2OrderBook;

pub struct AggressiveBreakoutEngine {
    pub min_breakout_bps: Fixed8,
    pub volume_skew_threshold: Fixed8,
    pub default_qty: Fixed8,
    pub stop_loss_bps: Fixed8,
    pub take_profit_bps: Fixed8,
    next_order_id: u64,
}

impl AggressiveBreakoutEngine {
    pub fn new(min_breakout_bps: Fixed8, default_qty: Fixed8) -> Self {
        Self {
            min_breakout_bps,
            volume_skew_threshold: Fixed8::from_f64(1.1),
            default_qty,
            stop_loss_bps: Fixed8::from_f64(1.0),
            take_profit_bps: Fixed8::from_f64(1.5),
            next_order_id: 3_000_000,
        }
    }

    pub fn evaluate(
        &mut self,
        book: &L2OrderBook,
        recent_mids: &[Fixed8],
        now_ns: TimestampNs,
    ) -> Option<OrderIntent> {
        if recent_mids.len() < 3 {
            return None;
        }

        let curr_mid = book.mid_price()?;
        let ref_mid = recent_mids[0];
        if ref_mid.is_zero() {
            return None;
        }

        let price_change_bps = ((curr_mid - ref_mid) / ref_mid) * Fixed8::from_f64(10_000.0);
        let bid_depth: Fixed8 = book.bids().iter().take(5).fold(Fixed8::ZERO, |acc, l| acc + l.qty);
        let ask_depth: Fixed8 = book.asks().iter().take(5).fold(Fixed8::ZERO, |acc, l| acc + l.qty);

        if bid_depth.is_zero() || ask_depth.is_zero() {
            return None;
        }

        let order_id = self.next_order_id;

        // Bullish breakout
        if price_change_bps >= self.min_breakout_bps && (bid_depth / ask_depth) >= self.volume_skew_threshold {
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

        // Bearish breakdown
        if price_change_bps <= -self.min_breakout_bps && (ask_depth / bid_depth) >= self.volume_skew_threshold {
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
