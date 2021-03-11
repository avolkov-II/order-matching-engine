//! High-performance limit order book and matching engine.
//!
//! This crate implements a price-time priority matching engine for
//! limit order books, designed for low-latency trading applications.
//! It uses lock-free data structures from Crossbeam for concurrent
//! access and Tokio for async networking support.

pub mod order;
pub mod book;
pub mod engine;
pub mod types;
pub mod error;

pub use order::*;
pub use book::*;
pub use engine::*;
pub use types::*;
pub use error::*;
