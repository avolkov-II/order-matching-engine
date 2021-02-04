use crate::error::{OrderError, OrderResult};
use crate::types::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A single order in the system.
///
/// Orders are immutable after creation except for their remaining
/// quantity, which decreases as fills occur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// Unique order identifier.
    pub id: OrderId,
    /// Order side (buy or sell).
    pub side: Side,
    /// Limit price. None for market orders (not implemented here).
    pub price: Price,
    /// Initial quantity.
    pub quantity: Quantity,
    /// Remaining unfilled quantity.
    pub remaining: Quantity,
    /// Order type.
    pub order_type: OrderType,
    /// Current status.
    pub status: OrderStatus,
    /// Timestamp when the order was created (nanoseconds).
    pub timestamp: Timestamp,
}

impl Order {
    /// Create a new limit order.
    pub fn new_limit(
        id: OrderId,
        side: Side,
        price: Price,
        quantity: Quantity,
    ) -> OrderResult<Self> {
        if price.ticks() <= 0 {
            return Err(OrderError::InvalidPrice(price.ticks()));
        }
        if quantity == 0 {
            return Err(OrderError::InvalidQuantity(quantity));
        }

        Ok(Order {
            id,
            side,
            price,
            quantity,
            remaining: quantity,
            order_type: OrderType::Limit,
            status: OrderStatus::Active,
            timestamp: Utc::now().timestamp_nanos(),
        })
    }

    /// Create a new immediate-or-cancel order.
    pub fn new_ioc(id: OrderId, side: Side, price: Price, quantity: Quantity) -> OrderResult<Self> {
        let mut order = Self::new_limit(id, side, price, quantity)?;
        order.order_type = OrderType::ImmediateOrCancel;
        Ok(order)
    }

    /// Create a new fill-or-kill order.
    pub fn new_fok(id: OrderId, side: Side, price: Price, quantity: Quantity) -> OrderResult<Self> {
        let mut order = Self::new_limit(id, side, price, quantity)?;
        order.order_type = OrderType::FillOrKill;
        Ok(order)
    }

    /// Returns the filled quantity.
    pub fn filled_quantity(&self) -> Quantity {
        self.quantity - self.remaining
    }

    /// Returns true if the order is fully filled.
    pub fn is_filled(&self) -> bool {
        self.remaining == 0
    }

    /// Returns true if the order is still active on the book.
    pub fn is_active(&self) -> bool {
        matches!(self.status, OrderStatus::Active | OrderStatus::PartiallyFilled)
    }

    /// Reduce the remaining quantity by the given amount (a fill).
    /// Returns the amount actually filled.
    pub fn fill(&mut self, amount: Quantity) -> Quantity {
        let fill_amount = amount.min(self.remaining);
        self.remaining -= fill_amount;

        if self.remaining == 0 {
            self.status = OrderStatus::Filled;
        } else {
            self.status = OrderStatus::PartiallyFilled;
        }

        fill_amount
    }

    /// Cancel the order. Returns an error if already filled or cancelled.
    pub fn cancel(&mut self) -> OrderResult<()> {
        match self.status {
            OrderStatus::Filled => Err(OrderError::AlreadyFilled(self.id)),
            OrderStatus::Cancelled => Err(OrderError::AlreadyCancelled(self.id)),
            _ => {
                self.status = OrderStatus::Cancelled;
                Ok(())
            }
        }
    }
}

impl fmt::Display for Order {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Order(id={}, side={}, price={}, qty={}/{}, status={:?})",
            self.id,
            self.side,
            self.price,
            self.remaining,
            self.quantity,
            self.status
        )
    }
}

impl PartialEq for Order {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Order {}

impl PartialOrd for Order {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Price-time priority: compare by price first, then by timestamp.
        // For buys, higher price = higher priority.
        // For sells, lower price = higher priority.
        match self.side {
            Side::Buy => match self.price.cmp(&other.price).reverse() {
                std::cmp::Ordering::Equal => Some(self.timestamp.cmp(&other.timestamp)),
                other => Some(other),
            },
            Side::Sell => match self.price.cmp(&other.price) {
                std::cmp::Ordering::Equal => Some(self.timestamp.cmp(&other.timestamp)),
                other => Some(other),
            },
        }
    }
}

impl Ord for Order {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_limit_order() {
        let order = Order::new_limit(1, Side::Buy, Price::new(100), 1000).unwrap();
        assert_eq!(order.id, 1);
        assert_eq!(order.side, Side::Buy);
        assert_eq!(order.price.ticks(), 100);
        assert_eq!(order.quantity, 1000);
        assert_eq!(order.remaining, 1000);
        assert!(order.is_active());
    }

    #[test]
    fn test_fill_order() {
        let mut order = Order::new_limit(1, Side::Buy, Price::new(100), 1000).unwrap();
        let filled = order.fill(400);
        assert_eq!(filled, 400);
        assert_eq!(order.remaining, 600);
        assert_eq!(order.status, OrderStatus::PartiallyFilled);

        let filled = order.fill(600);
        assert_eq!(filled, 600);
        assert_eq!(order.remaining, 0);
        assert_eq!(order.status, OrderStatus::Filled);
    }

    #[test]
    fn test_cancel_order() {
        let mut order = Order::new_limit(1, Side::Buy, Price::new(100), 1000).unwrap();
        assert!(order.cancel().is_ok());
        assert_eq!(order.status, OrderStatus::Cancelled);
        assert!(order.cancel().is_err()); // double cancel
    }

    #[test]
    fn test_invalid_price() {
        let result = Order::new_limit(1, Side::Buy, Price::new(0), 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_quantity() {
        let result = Order::new_limit(1, Side::Buy, Price::new(100), 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_price_time_priority_buy() {
        // Higher price should have higher priority for buys
        let order1 = Order::new_limit(1, Side::Buy, Price::new(100), 1000).unwrap();
        let order2 = Order::new_limit(2, Side::Buy, Price::new(101), 1000).unwrap();
        assert!(order2 > order1); // order2 has higher price -> higher priority
    }

    #[test]
    fn test_price_time_priority_sell() {
        // Lower price should have higher priority for sells
        let order1 = Order::new_limit(1, Side::Sell, Price::new(101), 1000).unwrap();
        let order2 = Order::new_limit(2, Side::Sell, Price::new(100), 1000).unwrap();
        assert!(order2 > order1); // order2 has lower price -> higher priority
    }
}
