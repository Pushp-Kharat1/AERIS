use aeris_core::{BookUpdate, Fixed8, Side, TimestampNs};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradePrint {
    pub price: Fixed8,
    pub qty: Fixed8,
    /// true if buyer is maker (trade was aggressive sell crossing bid)
    /// false if buyer is taker (trade was aggressive buy crossing ask)
    pub is_buyer_maker: bool,
    pub timestamp_ns: TimestampNs,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidationPrint {
    pub price: Fixed8,
    pub qty: Fixed8,
    pub side: Side,
    pub notional: Fixed8,
    pub timestamp_ns: TimestampNs,
}

#[derive(Clone, Debug)]
pub enum MarketEvent {
    Depth(BookUpdate),
    Trade(TradePrint),
    Liquidation(LiquidationPrint),
}
