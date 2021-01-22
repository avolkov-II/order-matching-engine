# order-matching-engine

A high-performance limit order book and matching engine in Rust, supporting price-time priority. Built for low-latency trading applications.

## Features

- **Price-time priority matching** — orders are matched at the best price, and within each price level, earlier orders get priority
- **Lock-free trade event queue** — uses Crossbeam's `MsQueue` for wait-free publishing of trade events
- **Async trade notifications** — Tokio channel integration for async downstream processing
- **Order types**: Limit, Immediate-or-Cancel (IOC), Fill-or-Kill (FOK)
- **Order book snapshots** — full depth snapshots with price levels, quantities, and order counts
- **Comprehensive statistics** — orders processed, trades executed, volume, spreads, and more
- **No floating-point** — prices are stored as integer ticks to avoid precision issues

## Tech Stack

| Component | Library | Version |
|-----------|---------|---------|
| Language | Rust | 2018 edition |
| Async runtime | Tokio | 0.2 |
| Lock-free queues | Crossbeam | 0.8 |
| Serialization | Serde | 1.0 |
| Logging | env_logger | 0.8 |
| Error handling | thiserror | 1.0 |
| Benchmarking | Criterion | 0.3 |

## Prerequisites

- Rust toolchain (stable, 2018 edition)
- Cargo (comes with Rust)

## Installation

```bash
git clone https://github.com/avolkov/order-matching-engine.git
cd order-matching-engine
cargo build --release
```

## Usage

### Run the example

```bash
RUST_LOG=info cargo run --release
```

This runs a simulated order flow for a hypothetical AAPL book, showing trades and market state.

### Use as a library

Add to your `Cargo.toml`:

```toml
[dependencies]
order-matching-engine = { git = "https://github.com/avolkov/order-matching-engine" }
```

### Basic API example

```rust
use order_matching_engine::engine::MatchingEngine;
use order_matching_engine::order::Order;
use order_matching_engine::types::*;

fn main() {
    let mut engine = MatchingEngine::new();

    // Add a sell order at 100
    let sell = Order::new_limit(1, Side::Sell, Price::new(100), 100).unwrap();
    engine.process_order(sell).unwrap();

    // Add a matching buy order
    let buy = Order::new_limit(2, Side::Buy, Price::new(100), 50).unwrap();
    let result = engine.process_order(buy).unwrap();

    println!("Trades executed: {}", result.trades.len());
    println!("Best bid: {:?}", engine.best_bid());
    println!("Best ask: {:?}", engine.best_ask());
    println!("Spread: {:?}", engine.spread());
}
```

### Running benchmarks

```bash
cargo bench
```

## Architecture

```
                    ┌─────────────┐
                    │   Incoming   │
                    │   Orders     │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │  Matching   │
                    │   Engine    │
                    │             │
                    │  ┌───────┐  │
                    │  │Order  │  │
                    │  │ Book  │  │
                    │  └───────┘  │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
       ┌──────▼─────┐ ┌───▼────┐ ┌────▼─────┐
       │  Crossbeam  │ │ Tokio  │ │  Stats / │
       │  MsQueue    │ │ Channel│ │Snapshot   │
       │ (Trades)    │ │(Async) │ │           │
       └────────────┘ └────────┘ └──────────┘
```

### Key design decisions

- **Single-threaded matching**: The core matching logic runs on a single thread, avoiding contention on the order book. Trade events are published through lock-free queues for downstream consumers.
- **BTreeMap for price levels**: Provides O(log n) insertion and lookup. Price levels are naturally sorted — bids in reverse, asks in ascending order.
- **VecDeque for time priority**: Each price level maintains a FIFO queue of orders. New orders go to the back; matching happens from the front.
- **Integer prices**: All prices are stored as `i64` ticks. This avoids floating-point rounding errors and makes comparison operations trivial.

## Project Structure

```
src/
├── lib.rs          # Crate root
├── main.rs         # CLI example
├── error.rs        # Error types
├── types.rs        # Core types (Side, Price, OrderType, etc.)
├── order.rs        # Order struct and operations
├── book.rs         # OrderBook and PriceLevel
└── engine.rs       # MatchingEngine with trade event publishing

benches/
└── matching_bench.rs  # Criterion benchmarks
```

## Running Tests

```bash
cargo test
```

## License

MIT License — see [LICENSE](LICENSE) for details.
