use aeris_core::{
    Fixed8, OrderIntent, OrderType, Side, TimeInForce, TimestampNs,
};
use aeris_feed::L2OrderBook;
use aeris_risk::{MetaLabelGate, MetaLabelGateConfig};

pub struct MicroMomentumEngine {
    pub divergence_hurdle_bps: Fixed8,
    pub meta_gate: MetaLabelGate,
    pub default_order_qty: Fixed8,
    next_order_id: u64,
}

impl MicroMomentumEngine {
    pub fn new(divergence_hurdle_bps: Fixed8, default_order_qty: Fixed8) -> Self {
        Self {
            divergence_hurdle_bps,
            meta_gate: MetaLabelGate::new(MetaLabelGateConfig::default()),
            default_order_qty,
            next_order_id: 2_000_000,
        }
    }

    pub fn evaluate_signal(
        &mut self,
        book: &L2OrderBook,
        macro_bias: u8,
        now_ns: TimestampNs,
    ) -> Option<OrderIntent> {
        let div_bps = book.micro_price_divergence_bps()?;
        let abs_div = div_bps.abs();

        if abs_div < self.divergence_hurdle_bps {
            return None;
        }

        let is_bullish = div_bps > Fixed8::ZERO;
        let side = if is_bullish { Side::Buy } else { Side::Sell };

        // Determine Win Probability p_hat conditioned on Macro Parent Model:
        // macro_bias 1 = SUPPLY_SQUEEZE (Bullish)
        // macro_bias 2 = DISTRIBUTION (Bearish)
        // macro_bias 0 or other = NEUTRAL
        let p_hat = match (is_bullish, macro_bias) {
            (true, 1) => Fixed8::from_f64(0.75),   // Bullish div + Bullish macro
            (false, 2) => Fixed8::from_f64(0.75),  // Bearish div + Bearish macro
            (true, 2) => Fixed8::from_f64(0.45),   // Bullish div against Bearish macro (blocked by hurdle)
            (false, 1) => Fixed8::from_f64(0.45),  // Bearish div against Bullish macro (blocked by hurdle)
            _ => Fixed8::from_f64(0.65),           // Neutral macro bias
        };

        let price = if is_bullish {
            book.best_ask()?.price
        } else {
            book.best_bid()?.price
        };

        let order_id = self.next_order_id;
        self.next_order_id += 1;

        let base_intent = OrderIntent {
            client_order_id: order_id,
            venue: book.venue,
            symbol: book.symbol,
            side,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price,
            qty: self.default_order_qty,
            created_at_ns: now_ns,
            parent_leg_id: None,
        };

        // Pass to Meta-Label Gate for win probability hurdle and Kelly fractional sizing
        self.meta_gate.evaluate_and_size(&base_intent, p_hat).ok()
    }
}
