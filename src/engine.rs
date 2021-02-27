use crate::book::{MatchResult, OrderBook, TradeEvent};
use crate::error::OrderResult;
use crate::order::Order;
use crate::types::*;
use crossbeam::sync::MsQueue;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Configuration for the matching engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Name of this engine instance (e.g., instrument symbol).
    pub symbol: String,
    /// Capacity of the trade event queue.
    pub trade_queue_capacity: usize,
    /// Whether to log all matches.
    pub log_trades: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            symbol: "DEFAULT".to_string(),
            trade_queue_capacity: 10_000,
            log_trades: true,
        }
    }
}

/// The core matching engine that processes orders against the order book.
///
/// This engine is designed to be used from a single thread for the
/// matching logic itself, while trade events are published through
/// lock-free queues for downstream consumers.
pub struct MatchingEngine {
    /// Configuration.
    config: EngineConfig,
    /// The order book instance.
    book: OrderBook,
    /// Lock-free queue for trade events (Crossbeam).
    trade_queue: Arc<MsQueue<TradeEvent>>,
    /// Tokio sender for notifying consumers of new trades.
    trade_notifier: Option<mpsc::UnboundedSender<TradeEvent>>,
    /// Total number of orders processed.
    orders_processed: AtomicU64,
    /// Total number of trades executed.
    trades_executed: AtomicU64,
    /// Total volume traded.
    volume_traded: AtomicU64,
}

impl MatchingEngine {
    /// Create a new matching engine with default configuration.
    pub fn new() -> Self {
        let config = EngineConfig::default();
        let trade_queue = Arc::new(MsQueue::new());
        let book = OrderBook::with_channel(trade_queue.clone());

        MatchingEngine {
            config,
            book,
            trade_queue,
            trade_notifier: None,
            orders_processed: AtomicU64::new(0),
            trades_executed: AtomicU64::new(0),
            volume_traded: AtomicU64::new(0),
        }
    }

    /// Create a new matching engine with the given configuration.
    pub fn with_config(config: EngineConfig) -> Self {
        let trade_queue = Arc::new(MsQueue::new());
        let book = OrderBook::with_channel(trade_queue.clone());

        MatchingEngine {
            config,
            book,
            trade_queue,
            trade_notifier: None,
            orders_processed: AtomicU64::new(0),
            trades_executed: AtomicU64::new(0),
            volume_traded: AtomicU64::new(0),
        }
    }

    /// Attach a Tokio channel for async trade notifications.
    pub fn set_trade_notifier(&mut self, notifier: mpsc::UnboundedSender<TradeEvent>) {
        self.trade_notifier = Some(notifier);
    }

    /// Get a reference to the Crossbeam trade queue for direct consumption.
    pub fn trade_queue(&self) -> Arc<MsQueue<TradeEvent>> {
        self.trade_queue.clone()
    }

    /// Process an incoming order through the matching engine.
    ///
    /// This is the main entry point. It matches the order against
    /// the book, executes trades, and optionally adds remaining
    /// quantity to the book.
    pub fn process_order(&mut self, order: Order) -> OrderResult<MatchResult> {
        self.orders_processed
            .fetch_add(1, AtomicOrdering::Relaxed);

        debug!(
            "[{}] Processing order: {}",
            self.config.symbol, order
        );

        let result = self.book.match_order(order)?;

        self.trades_executed
            .fetch_add(result.trades.len() as u64, AtomicOrdering::Relaxed);

        for trade in &result.trades {
            self.volume_traded
                .fetch_add(trade.quantity, AtomicOrdering::Relaxed);

            if self.config.log_trades {
                info!(
                    "[{}] TRADE: {} {} @ {} (qty: {})",
                    self.config.symbol,
                    trade.side,
                    trade.quantity,
                    trade.price,
                    trade.quantity,
                );
            }

            // Forward to Tokio channel if configured
            if let Some(ref notifier) = self.trade_notifier {
                if let Err(e) = notifier.send(trade.clone()) {
                    warn!("Failed to send trade event: {}", e);
                }
            }
        }

        Ok(result)
    }

    /// Cancel an order by ID.
    pub fn cancel_order(&mut self, order_id: OrderId) -> OrderResult<Order> {
        let mut order = self.book.remove(order_id)?;
        order.cancel()?;
        debug!(
            "[{}] Cancelled order: {}",
            self.config.symbol, order_id
        );
        Ok(order)
    }

    /// Get the current best bid price.
    pub fn best_bid(&self) -> Option<Price> {
        self.book.best_bid()
    }

    /// Get the current best ask price.
    pub fn best_ask(&self) -> Option<Price> {
        self.book.best_ask()
    }

    /// Get the current spread.
    pub fn spread(&self) -> Option<i64> {
        self.book.spread()
    }

    /// Get the mid price.
    pub fn mid_price(&self) -> Option<Price> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(Price::new((bid.ticks() + ask.ticks()) / 2)),
            _ => None,
        }
    }

    /// Get total volume on the bid side.
    pub fn total_bid_volume(&self) -> Quantity {
        self.book
            .bid_levels()
            .iter()
            .map(|(_, l)| l.total_quantity)
            .sum()
    }

    /// Get total volume on the ask side.
    pub fn total_ask_volume(&self) -> Quantity {
        self.book
            .ask_levels()
            .iter()
            .map(|(_, l)| l.total_quantity)
            .sum()
    }

    /// Get the current order book snapshot.
    pub fn book_snapshot(&self) -> OrderBookSnapshot {
        let bids = self
            .book
            .bid_levels()
            .iter()
            .map(|(price, level)| LevelSnapshot {
                price: **price,
                quantity: level.total_quantity,
                order_count: level.len(),
            })
            .collect();

        let asks = self
            .book
            .ask_levels()
            .iter()
            .map(|(price, level)| LevelSnapshot {
                price: **price,
                quantity: level.total_quantity,
                order_count: level.len(),
            })
            .collect();

        OrderBookSnapshot {
            symbol: self.config.symbol.clone(),
            bids,
            asks,
            timestamp: chrono::Utc::now().timestamp_nanos(),
        }
    }

    /// Get engine statistics.
    pub fn stats(&self) -> EngineStats {
        EngineStats {
            symbol: self.config.symbol.clone(),
            orders_processed: self.orders_processed.load(AtomicOrdering::Relaxed),
            trades_executed: self.trades_executed.load(AtomicOrdering::Relaxed),
            volume_traded: self.volume_traded.load(AtomicOrdering::Relaxed),
            active_orders: self.book.order_count() as u64,
            best_bid: self.best_bid(),
            best_ask: self.best_ask(),
            spread: self.spread(),
        }
    }
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of a single price level.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LevelSnapshot {
    pub price: Price,
    pub quantity: Quantity,
    pub order_count: usize,
}

/// A snapshot of the entire order book.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderBookSnapshot {
    pub symbol: String,
    pub bids: Vec<LevelSnapshot>,
    pub asks: Vec<LevelSnapshot>,
    pub timestamp: Timestamp,
}

/// Engine statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EngineStats {
    pub symbol: String,
    pub orders_processed: u64,
    pub trades_executed: u64,
    pub volume_traded: u64,
    pub active_orders: u64,
    pub best_bid: Option<Price>,
    pub best_ask: Option<Price>,
    pub spread: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_basic_matching() {
        let mut engine = MatchingEngine::new();

        // Add sell order
        let sell = Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap();
        let result = engine.process_order(sell).unwrap();
        assert!(result.remaining_order.is_some());
        assert_eq!(engine.stats().active_orders, 1);

        // Add matching buy order
        let buy = Order::new_limit(2, Side::Buy, Price::new(100), 100).unwrap();
        let result = engine.process_order(buy).unwrap();
        assert_eq!(result.trades.len(), 1);
        assert_eq!(engine.stats().trades_executed, 1);
        assert_eq!(engine.stats().volume_traded, 100);
        assert_eq!(engine.stats().active_orders, 0);
    }

    #[test]
    fn test_engine_mid_price() {
        let mut engine = MatchingEngine::new();

        engine
            .process_order(Order::new_limit(1, Side::Sell, Price::new(102), 100).unwrap())
            .unwrap();
        engine
            .process_order(Order::new_limit(2, Side::Buy, Price::new(100), 100).unwrap())
            .unwrap();

        assert_eq!(engine.mid_price(), Some(Price::new(101)));
    }

    #[test]
    fn test_engine_cancel() {
        let mut engine = MatchingEngine::new();

        engine
            .process_order(Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap())
            .unwrap();
        assert_eq!(engine.stats().active_orders, 1);

        engine.cancel_order(1).unwrap();
        assert_eq!(engine.stats().active_orders, 0);
    }

    #[test]
    fn test_engine_snapshot() {
        let mut engine = MatchingEngine::new();

        engine
            .process_order(Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap())
            .unwrap();
        engine
            .process_order(Order::new_limit(2, Side::Sell, Price::new(101), 200).unwrap())
            .unwrap();
        engine
            .process_order(Order::new_limit(3, Side::Buy, Price::new(99), 150).unwrap())
            .unwrap();

        let snapshot = engine.book_snapshot();
        assert_eq!(snapshot.bids.len(), 1);
        assert_eq!(snapshot.asks.len(), 2);
        assert_eq!(snapshot.bids[0].quantity, 150);
        assert_eq!(snapshot.asks[0].quantity, 100);
        assert_eq!(snapshot.asks[1].quantity, 200);
    }
}
