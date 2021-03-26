use log::{error, info};
use order_matching_engine::engine::{EngineConfig, MatchingEngine};
use order_matching_engine::order::Order;
use order_matching_engine::types::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

/// Simple CLI example demonstrating the matching engine.
///
/// This binary sets up a matching engine, processes a batch of
/// orders, and prints trade events and book state.
#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("Starting order matching engine...");

    let config = EngineConfig {
        symbol: "AAPL".to_string(),
        trade_queue_capacity: 100_000,
        log_trades: true,
    };

    let mut engine = MatchingEngine::with_config(config);

    // Set up a Tokio channel for async trade notifications
    let (trade_tx, mut trade_rx) = mpsc::unbounded_channel();
    engine.set_trade_notifier(trade_tx);

    // Spawn a consumer task for trade events
    let trade_consumer = tokio::spawn(async move {
        while let Some(trade) = trade_rx.recv().await {
            info!(
                "Trade: {} {} @ {} (qty: {})",
                trade.side, trade.quantity, trade.price, trade.quantity
            );
        }
    });

    // Spawn a task to periodically print market data
    let engine_arc = Arc::new(tokio::sync::Mutex::new(engine));
    let market_watcher = {
        let engine = engine_arc.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(2));
            loop {
                interval.tick().await;
                let eng = engine.lock().await;
                let stats = eng.stats();
                info!(
                    "Market: {} | Bid: {:?} | Ask: {:?} | Spread: {:?} | Trades: {} | Volume: {}",
                    stats.symbol,
                    stats.best_bid,
                    stats.best_ask,
                    stats.spread,
                    stats.trades_executed,
                    stats.volume_traded
                );
            }
        })
    };

    // Simulate some order flow
    let order_flow = {
        let engine = engine_arc.clone();
        tokio::spawn(async move {
            let orders = vec![
                // Add some sell liquidity
                Order::new_limit(1, Side::Sell, Price::new(150_00), 100).unwrap(),
                Order::new_limit(2, Side::Sell, Price::new(151_00), 200).unwrap(),
                Order::new_limit(3, Side::Sell, Price::new(152_00), 150).unwrap(),
                // Add some buy liquidity
                Order::new_limit(4, Side::Buy, Price::new(149_00), 100).unwrap(),
                Order::new_limit(5, Side::Buy, Price::new(148_00), 300).unwrap(),
                Order::new_limit(6, Side::Buy, Price::new(147_00), 200).unwrap(),
            ];

            for order in orders {
                let mut eng = engine.lock().await;
                match eng.process_order(order) {
                    Ok(result) => {
                        if !result.trades.is_empty() {
                            info!("Matched {} trades", result.trades.len());
                        }
                    }
                    Err(e) => error!("Order error: {}", e),
                }
                time::sleep(Duration::from_millis(10)).await;
            }

            // Now send a matching order
            let aggressive_buy = Order::new_limit(7, Side::Buy, Price::new(151_00), 250).unwrap();
            {
                let mut eng = engine.lock().await;
                match eng.process_order(aggressive_buy) {
                    Ok(result) => {
                        info!(
                            "Aggressive order: {} trades, remaining: {}",
                            result.trades.len(),
                            result
                                .remaining_order
                                .as_ref()
                                .map(|o| o.remaining.to_string())
                                .unwrap_or_else(|| "none".to_string())
                        );
                    }
                    Err(e) => error!("Order error: {}", e),
                }
            }

            // Print final snapshot
            {
                let eng = engine.lock().await;
                let snapshot = eng.book_snapshot();
                info!("=== Final Book Snapshot ===");
                info!("Bids:");
                for level in &snapshot.bids {
                    info!("  {}: {} ({} orders)", level.price, level.quantity, level.order_count);
                }
                info!("Asks:");
                for level in &snapshot.asks {
                    info!("  {}: {} ({} orders)", level.price, level.quantity, level.order_count);
                }
                let stats = eng.stats();
                info!("Stats: {} orders, {} trades, {} volume",
                    stats.orders_processed, stats.trades_executed, stats.volume_traded);
            }
        })
    };

    // Wait for order flow to complete
    let _ = order_flow.await;

    // Give the consumer a moment to process remaining events
    time::sleep(Duration::from_millis(100)).await;

    // Shutdown the market watcher
    market_watcher.abort();
    trade_consumer.abort();

    info!("Engine shutdown complete.");
}
