use crate::types::{Fixed8, OrderType, Side, Symbol, TimeInForce, TimestampNs, Venue};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Fixed8,
    pub qty: Fixed8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookUpdate {
    pub venue: Venue,
    pub symbol: Symbol,
    pub timestamp_ns: TimestampNs,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub is_snapshot: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeTick {
    pub venue: Venue,
    pub symbol: Symbol,
    pub timestamp_ns: TimestampNs,
    pub price: Fixed8,
    pub qty: Fixed8,
    pub is_buyer_maker: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderIntent {
    pub client_order_id: u64,
    pub venue: Venue,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub price: Fixed8,
    pub qty: Fixed8,
    pub created_at_ns: TimestampNs,
    pub parent_leg_id: Option<u64>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub client_order_id: u64,
    pub venue: Venue,
    pub symbol: Symbol,
    pub status: ExecutionStatus,
    pub last_fill_price: Fixed8,
    pub last_fill_qty: Fixed8,
    pub cumulative_qty: Fixed8,
    pub leaves_qty: Fixed8,
    pub fee_paid: Fixed8,
    pub tds_paid: Fixed8,
    pub updated_at_ns: TimestampNs,
}
