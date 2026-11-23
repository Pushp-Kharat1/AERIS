use aeris_core::{BookLevel, BookUpdate, Fixed8, Side, Symbol, TimestampNs, Venue};

use crate::events::{LiquidationPrint, MarketEvent, TradePrint};

pub struct SyntheticMarketGenerator {
    pub venue: Venue,
    pub symbol: Symbol,
    pub current_mid: Fixed8,
    pub current_time_ns: TimestampNs,
    pub tick_size: Fixed8,
}

impl SyntheticMarketGenerator {
    pub fn new(venue: Venue, symbol: Symbol, initial_mid: Fixed8) -> Self {
        Self {
            venue,
            symbol,
            current_mid: initial_mid,
            current_time_ns: 1_700_000_000_000_000_000,
            tick_size: Fixed8::from_f64(0.1),
        }
    }

    /// Advance time by milliseconds
    pub fn advance_time(&mut self, ms: u64) -> TimestampNs {
        self.current_time_ns += ms * 1_000_000;
        self.current_time_ns
    }

    /// Generate an L2 depth update around current mid
    pub fn generate_l2_update(&mut self, spread_bps: f64, depth_per_level: f64) -> MarketEvent {
        let half_spread = self.current_mid * Fixed8::from_f64(spread_bps / 20000.0);
        let best_bid = self.current_mid - half_spread;
        let best_ask = self.current_mid + half_spread;
        let qty = Fixed8::from_f64(depth_per_level);

        let mut bids = Vec::new();
        let mut asks = Vec::new();

        for i in 0..5 {
            let offset = self.tick_size * Fixed8::from_int(i as i64 * 5);
            bids.push(BookLevel {
                price: best_bid - offset,
                qty,
            });
            asks.push(BookLevel {
                price: best_ask + offset,
                qty,
            });
        }

        MarketEvent::Depth(BookUpdate {
            venue: self.venue,
            symbol: self.symbol,
            timestamp_ns: self.current_time_ns,
            bids,
            asks,
            is_snapshot: true,
        })
    }

    /// Generate a normal trade print at bid or ask
    pub fn generate_trade(&mut self, is_buyer_maker: bool, qty: f64) -> MarketEvent {
        let half_spread = self.current_mid * Fixed8::from_f64(0.0001); // 1 bp
        let price = if is_buyer_maker {
            self.current_mid - half_spread // Sell hitting bid
        } else {
            self.current_mid + half_spread // Buy hitting ask
        };

        MarketEvent::Trade(TradePrint {
            price,
            qty: Fixed8::from_f64(qty),
            is_buyer_maker,
            timestamp_ns: self.current_time_ns,
        })
    }

    /// Generate a violent liquidation cascade (multiple market sells crashing price)
    pub fn generate_liquidation_cascade(&mut self, cascade_depth_bps: f64, notional: f64) -> Vec<MarketEvent> {
        let mut events = Vec::new();
        let drop = self.current_mid * Fixed8::from_f64(cascade_depth_bps / 10000.0);
        let target_mid = self.current_mid - drop;
        let steps = 5;
        let step_drop = drop / Fixed8::from_int(steps as i64);
        let step_qty = Fixed8::from_f64(notional / steps as f64) / self.current_mid;

        for _ in 0..steps {
            self.advance_time(50);
            self.current_mid = self.current_mid - step_drop;

            // Emit deep liquidation trade
            events.push(MarketEvent::Liquidation(LiquidationPrint {
                price: self.current_mid,
                qty: step_qty,
                side: Side::Sell,
                notional: Fixed8::from_f64(notional / steps as f64),
                timestamp_ns: self.current_time_ns,
            }));

            // Emit collapsed depth update
            events.push(self.generate_l2_update(3.0, 0.2));
        }

        self.current_mid = target_mid;
        events
    }
}
