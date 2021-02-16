use crate::error::{OrderError, OrderResult};
use crate::order::Order;
use crate::types::*;
use crossbeam::sync::MsQueue;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

/// A price level in the order book, containing all orders at that price.
///
/// Orders are stored in a FIFO queue to maintain time priority at each
/// price level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    /// The price for this level.
    pub price: Price,
    /// Total quantity available at this price level.
    pub total_quantity: Quantity,
    /// Queue of orders at this price level (FIFO = time priority).
    #[serde(skip)]
    orders: VecDeque<Order>,
}

impl PriceLevel {
    fn new(price: Price) -> Self {
        PriceLevel {
            price,
            total_quantity: 0,
            orders: VecDeque::new(),
        }
    }

    /// Add an order to the back of the queue (time priority).
    fn push(&mut self, order: Order) {
        self.total_quantity += order.remaining;
        self.orders.push_back(order);
    }

    /// Peek at the front order without removing it.
    fn front(&self) -> Option<&Order> {
        self.orders.front()
    }

    /// Remove and return the front order.
    fn pop_front(&mut self) -> Option<Order> {
        if let Some(order) = self.orders.pop_front() {
            self.total_quantity -= order.remaining;
            Some(order)
        } else {
            None
        }
    }

    /// Returns true if this price level has no orders.
    fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Number of orders at this level.
    fn len(&self) -> usize {
        self.orders.len()
    }
}

/// The limit order book, maintaining separate sides for bids and asks.
///
/// Bids are stored in ascending order by price internally (BTreeMap default),
/// but accessed highest-first through reverse iteration.
/// Asks are stored in ascending order (lowest price first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    /// All bids (buy orders), keyed by price.
    /// BTreeMap sorts ascending; we iterate in reverse for best-bid-first.
    #[serde(skip)]
    bids: BTreeMap<Price, PriceLevel>,
    /// All asks (sell orders), keyed by price, sorted ascending.
    #[serde(skip)]
    asks: BTreeMap<Price, PriceLevel>,
    /// Map of order ID to order for O(1) lookups.
    #[serde(skip)]
    order_map: HashMap<OrderId, Order>,
    /// Sequence counter for order IDs.
    next_id: OrderId,
    /// Channel for broadcasting trade events.
    #[serde(skip)]
    trade_channel: Option<Arc<MsQueue<TradeEvent>>>,
}

/// A trade event emitted when orders are matched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    /// Order ID of the taker.
    pub taker_order_id: OrderId,
    /// Order ID of the maker.
    pub maker_order_id: OrderId,
    /// Side of the taker.
    pub side: Side,
    /// Price at which the trade occurred.
    pub price: Price,
    /// Quantity traded.
    pub quantity: Quantity,
    /// Timestamp of the trade.
    pub timestamp: Timestamp,
}

impl OrderBook {
    /// Create a new empty order book.
    pub fn new() -> Self {
        OrderBook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            order_map: HashMap::new(),
            next_id: 1,
            trade_channel: None,
        }
    }

    /// Create a new order book with a trade event channel.
    pub fn with_channel(channel: Arc<MsQueue<TradeEvent>>) -> Self {
        let mut book = Self::new();
        book.trade_channel = Some(channel);
        book
    }

    /// Set the trade event channel.
    pub fn set_channel(&mut self, channel: Arc<MsQueue<TradeEvent>>) {
        self.trade_channel = Some(channel);
    }

    /// Get the next order ID and increment the counter.
    pub fn next_order_id(&mut self) -> OrderId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Add an order to the book without matching.
    /// This is used internally after matching is complete.
    fn add_to_book(&mut self, order: Order) {
        let price = order.price;
        let side = order.side;

        let level = match side {
            Side::Buy => self.bids.entry(price).or_insert_with(|| PriceLevel::new(price)),
            Side::Sell => self.asks.entry(price).or_insert_with(|| PriceLevel::new(price)),
        };

        level.push(order.clone());
        self.order_map.insert(order.id, order);
    }

    /// Remove an order from the book by ID.
    pub fn remove(&mut self, order_id: OrderId) -> OrderResult<Order> {
        let order = self
            .order_map
            .get(&order_id)
            .ok_or(OrderError::NotFound(order_id))?
            .clone();

        let price = order.price;
        let side = order.side;

        let level = match side {
            Side::Buy => self.bids.get_mut(&price),
            Side::Sell => self.asks.get_mut(&price),
        };

        if let Some(level) = level {
            // Remove the specific order from the deque (O(n) at the price level).
            let len_before = level.len();
            level.orders.retain(|o| o.id != order_id);
            let len_after = level.len();
            let removed_count = len_before - len_after;

            if removed_count > 0 {
                level.total_quantity -= order.remaining;
            }

            // Clean up empty price levels
            if level.is_empty() {
                match side {
                    Side::Buy => self.bids.remove(&price),
                    Side::Sell => self.asks.remove(&price),
                };
            }
        }

        self.order_map.remove(&order_id);
        Ok(order)
    }

    /// Get a reference to an order by ID.
    pub fn get_order(&self, order_id: OrderId) -> Option<&Order> {
        self.order_map.get(&order_id)
    }

    /// Get the best bid (highest buy price).
    pub fn best_bid(&self) -> Option<Price> {
        self.bids.keys().next_back().copied()
    }

    /// Get the best ask (lowest sell price).
    pub fn best_ask(&self) -> Option<Price> {
        self.asks.keys().next().copied()
    }

    /// Get the spread (best ask - best bid).
    pub fn spread(&self) -> Option<i64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.ticks() - bid.ticks()),
            _ => None,
        }
    }

    /// Get total bid quantity at a given price level.
    pub fn bid_quantity(&self, price: &Price) -> Quantity {
        self.bids
            .get(price)
            .map(|l| l.total_quantity)
            .unwrap_or(0)
    }

    /// Get total ask quantity at a given price level.
    pub fn ask_quantity(&self, price: &Price) -> Quantity {
        self.asks
            .get(price)
            .map(|l| l.total_quantity)
            .unwrap_or(0)
    }

    /// Get all bid price levels (sorted descending by price).
    pub fn bid_levels(&self) -> Vec<(&Price, &PriceLevel)> {
        self.bids.iter().rev().collect()
    }

    /// Get all ask price levels (sorted ascending by price).
    pub fn ask_levels(&self) -> Vec<(&Price, &PriceLevel)> {
        self.asks.iter().collect()
    }

    /// Number of active orders in the book.
    pub fn order_count(&self) -> usize {
        self.order_map.len()
    }

    /// Emit a trade event through the channel, if configured.
    fn emit_trade(&self, event: TradeEvent) {
        if let Some(ref channel) = self.trade_channel {
            channel.push(event);
        }
    }

    /// Try to match an incoming order against the book.
    ///
    /// Matching logic:
    /// - A buy order matches against the ask side at prices <= buy price.
    ///   Asks are sorted ascending, so we iterate from the lowest ask upward.
    /// - A sell order matches against the bid side at prices >= sell price.
    ///   Bids are sorted ascending in the BTreeMap, so we iterate in reverse
    ///   (highest bid first) down to the sell price.
    ///
    /// Within each price level, orders are matched in FIFO order (time priority).
    pub fn match_order(&mut self, mut order: Order) -> OrderResult<MatchResult> {
        let mut trades = Vec::new();
        let is_taker_buy = order.side == Side::Buy;

        if is_taker_buy {
            // Buy order: match against asks at prices <= buy price.
            // Asks are sorted ascending (lowest first), which is exactly
            // what we want — match at the best (lowest) ask first.
            let crossing_prices: Vec<Price> = self
                .asks
                .range(Price::new(0)..=order.price)
                .map(|(p, _)| *p)
                .collect();

            for price in crossing_prices {
                if order.remaining == 0 {
                    break;
                }

                let level = match self.asks.get_mut(&price) {
                    Some(l) => l,
                    None => continue,
                };

                // Match against orders at this price level in FIFO order
                while !level.is_empty() && order.remaining > 0 {
                    let mut maker_order = match level.pop_front() {
                        Some(o) => o,
                        None => break,
                    };

                    let fill_qty = order.remaining.min(maker_order.remaining);
                    maker_order.fill(fill_qty);
                    order.fill(fill_qty);

                    let trade = TradeEvent {
                        taker_order_id: order.id,
                        maker_order_id: maker_order.id,
                        side: order.side,
                        price,
                        quantity: fill_qty,
                        timestamp: maker_order.timestamp.max(order.timestamp),
                    };
                    trades.push(trade.clone());
                    self.emit_trade(trade);

                    if maker_order.is_filled() {
                        self.order_map.remove(&maker_order.id);
                    } else {
                        // Partially filled maker goes back to front of queue
                        level.orders.push_front(maker_order.clone());
                        level.total_quantity += maker_order.remaining;
                        self.order_map.insert(maker_order.id, maker_order);
                        break;
                    }
                }

                // Clean up empty price levels
                if level.is_empty() {
                    self.asks.remove(&price);
                }
            }
        } else {
            // Sell order: match against bids at prices >= sell price.
            // Bids are sorted ascending in BTreeMap, so we need to iterate
            // in reverse (highest bid first) down to the sell price.
            let crossing_prices: Vec<Price> = self
                .bids
                .range(order.price..)
                .map(|(p, _)| *p)
                .rev() // highest bid first
                .collect();

            for price in crossing_prices {
                if order.remaining == 0 {
                    break;
                }

                let level = match self.bids.get_mut(&price) {
                    Some(l) => l,
                    None => continue,
                };

                // Match against orders at this price level in FIFO order
                while !level.is_empty() && order.remaining > 0 {
                    let mut maker_order = match level.pop_front() {
                        Some(o) => o,
                        None => break,
                    };

                    let fill_qty = order.remaining.min(maker_order.remaining);
                    maker_order.fill(fill_qty);
                    order.fill(fill_qty);

                    let trade = TradeEvent {
                        taker_order_id: order.id,
                        maker_order_id: maker_order.id,
                        side: order.side,
                        price,
                        quantity: fill_qty,
                        timestamp: maker_order.timestamp.max(order.timestamp),
                    };
                    trades.push(trade.clone());
                    self.emit_trade(trade);

                    if maker_order.is_filled() {
                        self.order_map.remove(&maker_order.id);
                    } else {
                        // Partially filled maker goes back to front of queue
                        level.orders.push_front(maker_order.clone());
                        level.total_quantity += maker_order.remaining;
                        self.order_map.insert(maker_order.id, maker_order);
                        break;
                    }
                }

                // Clean up empty price levels
                if level.is_empty() {
                    self.bids.remove(&price);
                }
            }
        }

        let is_consumed = order.remaining == 0;

        // Handle order type semantics
        let should_add_to_book = match order.order_type {
            OrderType::Limit => !is_consumed,
            OrderType::ImmediateOrCancel => {
                // IOC: never rests on the book, cancel remaining
                if !is_consumed {
                    order.status = OrderStatus::Cancelled;
                }
                false
            }
            OrderType::FillOrKill => {
                // FOK: must fill entirely or be cancelled.
                // If not fully filled, cancel and clear trades.
                if !is_consumed {
                    order.status = OrderStatus::Cancelled;
                    trades.clear();
                }
                false
            }
        };

        if should_add_to_book && order.is_active() {
            self.add_to_book(order.clone());
        }

        Ok(MatchResult {
            trades,
            remaining_order: if should_add_to_book && !is_consumed {
                Some(order)
            } else {
                None
            },
        })
    }
}

/// Result of matching an order against the book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    /// Trades that occurred during matching.
    pub trades: Vec<TradeEvent>,
    /// If the order was not fully consumed and should rest on the book.
    pub remaining_order: Option<Order>,
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_book() {
        let book = OrderBook::new();
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
        assert!(book.spread().is_none());
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn test_simple_match() {
        let mut book = OrderBook::new();

        // Add a sell order at 100
        let sell = Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap();
        let result = book.match_order(sell).unwrap();
        assert!(result.remaining_order.is_some());
        assert_eq!(book.order_count(), 1);

        // Add a buy order at 100 — should match
        let buy = Order::new_limit(2, Side::Buy, Price::new(100), 50).unwrap();
        let result = book.match_order(buy).unwrap();
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].quantity, 50);
        assert!(result.remaining_order.is_none());

        // Remaining sell order should still be on the book with 50 left
        assert_eq!(book.order_count(), 1);
        let remaining = book.get_order(1).unwrap();
        assert_eq!(remaining.remaining, 50);
    }

    #[test]
    fn test_buy_matches_best_ask_first() {
        let mut book = OrderBook::new();

        // Add sell orders: one at 101, one at 100 (better price)
        book.match_order(Order::new_limit(1, Side::Sell, Price::new(101), 100).unwrap())
            .unwrap();
        book.match_order(Order::new_limit(2, Side::Sell, Price::new(100), 100).unwrap())
            .unwrap();

        assert_eq!(book.best_ask(), Some(Price::new(100)));

        // Buy at 101 should match against the best ask (100) first
        let buy = Order::new_limit(3, Side::Buy, Price::new(101), 150).unwrap();
        let result = book.match_order(buy).unwrap();
        assert_eq!(result.trades.len(), 2);
        // First trade should be at 100 (best ask)
        assert_eq!(result.trades[0].price, Price::new(100));
        assert_eq!(result.trades[0].quantity, 100);
        // Second trade at 101
        assert_eq!(result.trades[1].price, Price::new(101));
        assert_eq!(result.trades[1].quantity, 50);
        assert!(result.remaining_order.is_none());
    }

    #[test]
    fn test_sell_matches_best_bid_first() {
        let mut book = OrderBook::new();

        // Add buy orders: one at 99, one at 100 (better price)
        book.match_order(Order::new_limit(1, Side::Buy, Price::new(99), 100).unwrap())
            .unwrap();
        book.match_order(Order::new_limit(2, Side::Buy, Price::new(100), 100).unwrap())
            .unwrap();

        assert_eq!(book.best_bid(), Some(Price::new(100)));

        // Sell at 99 should match against the best bid (100) first
        let sell = Order::new_limit(3, Side::Sell, Price::new(99), 150).unwrap();
        let result = book.match_order(sell).unwrap();
        assert_eq!(result.trades.len(), 2);
        // First trade should be at 100 (best bid)
        assert_eq!(result.trades[0].price, Price::new(100));
        assert_eq!(result.trades[0].quantity, 100);
        // Second trade at 99
        assert_eq!(result.trades[1].price, Price::new(99));
        assert_eq!(result.trades[1].quantity, 50);
        assert!(result.remaining_order.is_none());
    }

    #[test]
    fn test_cancel_order() {
        let mut book = OrderBook::new();

        book.match_order(Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap())
            .unwrap();
        assert_eq!(book.order_count(), 1);

        book.remove(1).unwrap();
        assert_eq!(book.order_count(), 0);
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn test_ioc_order() {
        let mut book = OrderBook::new();

        // Add a resting sell order
        book.match_order(Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap())
            .unwrap();

        // IOC buy that partially fills
        let ioc = Order::new_ioc(2, Side::Buy, Price::new(100), 200).unwrap();
        let result = book.match_order(ioc).unwrap();
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].quantity, 100);
        assert!(result.remaining_order.is_none()); // IOC never rests
        assert_eq!(book.order_count(), 0); // sell was fully filled
    }

    #[test]
    fn test_fok_order_cancels_if_not_fully_filled() {
        let mut book = OrderBook::new();

        // Add a resting sell order
        book.match_order(Order::new_limit(1, Side::Sell, Price::new(100), 50).unwrap())
            .unwrap();

        // FOK buy that can't fill entirely
        let fok = Order::new_fok(2, Side::Buy, Price::new(100), 100).unwrap();
        let result = book.match_order(fok).unwrap();
        // Should have no trades (FOK cancelled everything)
        assert_eq!(result.trades.len(), 0);
        assert!(result.remaining_order.is_none());
        // Original sell should still be there
        assert_eq!(book.order_count(), 1);
    }

    #[test]
    fn test_time_priority() {
        let mut book = OrderBook::new();

        // Add two sell orders at the same price
        book.match_order(Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap())
            .unwrap();
        book.match_order(Order::new_limit(2, Side::Sell, Price::new(100), 100).unwrap())
            .unwrap();

        // Buy 100 — should match against order 1 (earlier)
        let buy = Order::new_limit(3, Side::Buy, Price::new(100), 100).unwrap();
        let result = book.match_order(buy).unwrap();
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].maker_order_id, 1);

        // Buy another 100 — should match against order 2
        let buy2 = Order::new_limit(4, Side::Buy, Price::new(100), 100).unwrap();
        let result = book.match_order(buy2).unwrap();
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].maker_order_id, 2);
    }
}
