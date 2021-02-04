use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

/// The side of an order — buy or sell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    #[serde(rename = "buy")]
    Buy,
    #[serde(rename = "sell")]
    Sell,
}

impl Side {
    /// Returns the opposite side.
    pub fn opposite(&self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Buy => write!(f, "BUY"),
            Side::Sell => write!(f, "SELL"),
        }
    }
}

/// The type of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderType {
    /// A limit order that rests on the book until matched or cancelled.
    Limit,
    /// An immediate-or-cancel order that must fill immediately or is cancelled.
    ImmediateOrCancel,
    /// A fill-or-kill order that must fill entirely or is cancelled.
    FillOrKill,
}

/// The current status of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Order is active on the book.
    Active,
    /// Order has been fully filled.
    Filled,
    /// Order has been partially filled and is still active.
    PartiallyFilled,
    /// Order has been cancelled.
    Cancelled,
    /// Order was rejected.
    Rejected,
}

/// Represents a price level in the order book. Prices are stored as
/// integer ticks to avoid floating-point precision issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Price(pub i64);

impl Price {
    /// Create a new price from a raw tick value.
    pub fn new(ticks: i64) -> Self {
        Price(ticks)
    }

    /// Returns the raw tick value.
    pub fn ticks(&self) -> i64 {
        self.0
    }
}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A unique order identifier.
pub type OrderId = u64;

/// A quantity of shares/contracts.
pub type Quantity = u64;

/// Timestamp in nanoseconds since epoch.
pub type Timestamp = i64;
