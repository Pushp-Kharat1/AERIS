use aeris_core::{
    Fixed8, OrderIntent, OrderType, Side, Symbol, TimeInForce, TimestampNs, Venue,
};
use aeris_execution::FrictionModel;
use aeris_feed::L2OrderBook;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwoLegSpreadIntent {
    pub spread_id: u64,
    pub symbol: Symbol,
    pub leg_a_buy: OrderIntent,
    pub leg_b_sell: OrderIntent,
    pub gross_spread_bps: Fixed8,
    pub net_spread_bps: Fixed8,
    pub executable_qty: Fixed8,
}

#[derive(Clone, Debug)]
pub struct CrossVenueOpportunity {
    pub symbol: Symbol,
    pub buy_venue: Venue,
    pub sell_venue: Venue,
    pub buy_price: Fixed8,
    pub sell_price: Fixed8,
    pub max_qty: Fixed8,
    pub gross_spread_bps: Fixed8,
    pub net_spread_bps: Fixed8,
}

pub struct CrossVenueArbEngine {
    pub friction: FrictionModel,
    pub min_net_hurdle_bps: Fixed8,
    pub max_leg_notional: Fixed8,
    next_order_id: u64,
    next_spread_id: u64,
}

impl CrossVenueArbEngine {
    pub fn new(min_net_hurdle_bps: Fixed8, max_leg_notional: Fixed8) -> Self {
        Self {
            friction: FrictionModel::default(),
            min_net_hurdle_bps,
            max_leg_notional,
            next_order_id: 1_000_000,
            next_spread_id: 1,
        }
    }

    pub fn inspect_spread(
        &self,
        symbol: Symbol,
        book_a: &L2OrderBook,
        book_b: &L2OrderBook,
    ) -> Option<CrossVenueOpportunity> {
        let bbo_a_bid = book_a.best_bid()?;
        let bbo_a_ask = book_a.best_ask()?;
        let bbo_b_bid = book_b.best_bid()?;
        let bbo_b_ask = book_b.best_ask()?;

        // Direction 1: Buy on A, Sell on B
        let opp1 = self.calculate_opp(symbol, book_a.venue, book_b.venue, bbo_a_ask, bbo_b_bid);

        // Direction 2: Buy on B, Sell on A
        let opp2 = self.calculate_opp(symbol, book_b.venue, book_a.venue, bbo_b_ask, bbo_a_bid);

        match (opp1, opp2) {
            (Some(o1), Some(o2)) => {
                if o1.net_spread_bps >= o2.net_spread_bps {
                    Some(o1)
                } else {
                    Some(o2)
                }
            }
            (Some(o1), None) => Some(o1),
            (None, Some(o2)) => Some(o2),
            (None, None) => None,
        }
    }

    fn calculate_opp(
        &self,
        symbol: Symbol,
        buy_venue: Venue,
        sell_venue: Venue,
        buy_level: aeris_core::BookLevel,
        sell_level: aeris_core::BookLevel,
    ) -> Option<CrossVenueOpportunity> {
        if buy_level.price.is_zero() || sell_level.price <= buy_level.price {
            return None;
        }

        let gross_diff = sell_level.price - buy_level.price;
        let gross_bps = (gross_diff / buy_level.price) * Fixed8::from_f64(10000.0);

        // Friction:
        // Buy leg taker fee: 5 bps
        // Sell leg taker fee: 5 bps
        // Sell leg Section 194S TDS: 100 bps
        // Total friction: ~110 bps
        let taker_fees_bps = self.friction.taker_fee_bps + self.friction.taker_fee_bps;
        let tds_bps = self.friction.tds_rate * Fixed8::from_f64(10000.0);
        let total_friction_bps = taker_fees_bps + tds_bps;

        let net_spread_bps = gross_bps - total_friction_bps;

        let raw_qty = buy_level.qty.min(sell_level.qty);
        let notional = raw_qty * buy_level.price;
        let max_qty = if notional > self.max_leg_notional {
            self.max_leg_notional / buy_level.price
        } else {
            raw_qty
        };

        if max_qty.is_zero() {
            return None;
        }

        Some(CrossVenueOpportunity {
            symbol,
            buy_venue,
            sell_venue,
            buy_price: buy_level.price,
            sell_price: sell_level.price,
            max_qty,
            gross_spread_bps: gross_bps,
            net_spread_bps,
        })
    }

    pub fn generate_spread_intent(
        &mut self,
        opp: &CrossVenueOpportunity,
        now_ns: TimestampNs,
    ) -> Option<TwoLegSpreadIntent> {
        if opp.net_spread_bps < self.min_net_hurdle_bps || opp.max_qty.is_zero() {
            return None;
        }

        let spread_id = self.next_spread_id;
        self.next_spread_id += 1;

        let id_a = self.next_order_id;
        let id_b = self.next_order_id + 1;
        self.next_order_id += 2;

        let leg_a_buy = OrderIntent {
            client_order_id: id_a,
            venue: opp.buy_venue,
            symbol: opp.symbol,
            side: Side::Buy,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: opp.buy_price,
            qty: opp.max_qty,
            created_at_ns: now_ns,
            parent_leg_id: None,
        };

        let leg_b_sell = OrderIntent {
            client_order_id: id_b,
            venue: opp.sell_venue,
            symbol: opp.symbol,
            side: Side::Sell,
            order_type: OrderType::ImmediateOrCancel,
            time_in_force: TimeInForce::ImmediateOrCancel,
            price: opp.sell_price,
            qty: opp.max_qty,
            created_at_ns: now_ns,
            parent_leg_id: Some(id_a),
        };

        Some(TwoLegSpreadIntent {
            spread_id,
            symbol: opp.symbol,
            leg_a_buy,
            leg_b_sell,
            gross_spread_bps: opp.gross_spread_bps,
            net_spread_bps: opp.net_spread_bps,
            executable_qty: opp.max_qty,
        })
    }
}
