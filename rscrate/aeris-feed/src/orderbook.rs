use aeris_core::{BookLevel, BookUpdate, Fixed8, Side, Symbol, TimestampNs, Venue};

pub const MAX_DEPTH: usize = 50;

#[derive(Clone, Debug)]
pub struct L2OrderBook {
    pub venue: Venue,
    pub symbol: Symbol,
    pub last_update_ns: TimestampNs,
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
}

impl L2OrderBook {
    pub fn new(venue: Venue, symbol: Symbol) -> Self {
        Self {
            venue,
            symbol,
            last_update_ns: 0,
            bids: Vec::with_capacity(MAX_DEPTH),
            asks: Vec::with_capacity(MAX_DEPTH),
        }
    }

    pub fn apply_update(&mut self, update: &BookUpdate) {
        self.last_update_ns = update.timestamp_ns;
        if update.is_snapshot {
            self.bids.clear();
            self.asks.clear();
        }

        for bid in &update.bids {
            self.update_level(Side::Buy, bid.price, bid.qty);
        }

        for ask in &update.asks {
            self.update_level(Side::Sell, ask.price, ask.qty);
        }

        self.bids.sort_by(|a, b| b.price.cmp(&a.price));
        self.asks.sort_by(|a, b| a.price.cmp(&b.price));

        if self.bids.len() > MAX_DEPTH {
            self.bids.truncate(MAX_DEPTH);
        }
        if self.asks.len() > MAX_DEPTH {
            self.asks.truncate(MAX_DEPTH);
        }
    }

    fn update_level(&mut self, side: Side, price: Fixed8, qty: Fixed8) {
        let levels = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        if let Some(pos) = levels.iter().position(|l| l.price == price) {
            if qty.is_zero() {
                levels.remove(pos);
            } else {
                levels[pos].qty = qty;
            }
        } else if !qty.is_zero() {
            levels.push(BookLevel { price, qty });
        }
    }

    pub fn best_bid(&self) -> Option<BookLevel> {
        self.bids.first().copied()
    }

    pub fn best_ask(&self) -> Option<BookLevel> {
        self.asks.first().copied()
    }

    pub fn spread_bps(&self) -> Option<Fixed8> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        if bid.price.is_zero() || ask.price < bid.price {
            return None;
        }
        let spread = ask.price - bid.price;
        let bps = (spread / bid.price) * Fixed8::from_f64(10000.0);
        Some(bps)
    }

    pub fn mid_price(&self) -> Option<Fixed8> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        Some((bid.price + ask.price) / Fixed8::from_f64(2.0))
    }

    pub fn micro_price(&self) -> Option<Fixed8> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        let total_qty = bid.qty + ask.qty;
        if total_qty.is_zero() {
            return self.mid_price();
        }
        let weighted = (bid.qty * ask.price) + (ask.qty * bid.price);
        Some(weighted / total_qty)
    }

    pub fn micro_price_divergence_bps(&self) -> Option<Fixed8> {
        let mid = self.mid_price()?;
        let micro = self.micro_price()?;
        if mid.is_zero() {
            return None;
        }
        let diff = micro - mid;
        Some((diff / mid) * Fixed8::from_f64(10000.0))
    }

    pub fn ofi_top(&self, prev: &L2OrderBook) -> Fixed8 {
        let curr_bid = self.best_bid().unwrap_or(BookLevel { price: Fixed8::ZERO, qty: Fixed8::ZERO });
        let prev_bid = prev.best_bid().unwrap_or(BookLevel { price: Fixed8::ZERO, qty: Fixed8::ZERO });
        let curr_ask = self.best_ask().unwrap_or(BookLevel { price: Fixed8::ZERO, qty: Fixed8::ZERO });
        let prev_ask = prev.best_ask().unwrap_or(BookLevel { price: Fixed8::ZERO, qty: Fixed8::ZERO });

        let bid_delta = if curr_bid.price > prev_bid.price {
            curr_bid.qty
        } else if curr_bid.price == prev_bid.price {
            curr_bid.qty - prev_bid.qty
        } else {
            -prev_bid.qty
        };

        let ask_delta = if curr_ask.price < prev_ask.price {
            curr_ask.qty
        } else if curr_ask.price == prev_ask.price {
            curr_ask.qty - prev_ask.qty
        } else {
            -prev_ask.qty
        };

        bid_delta - ask_delta
    }
}
