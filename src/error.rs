use thiserror::Error;

/// Errors that can occur during order processing.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum OrderError {
    #[error("order not found: {0}")]
    NotFound(u64),

    #[error("duplicate order id: {0}")]
    DuplicateOrderId(u64),

    #[error("invalid price: {0}")]
    InvalidPrice(i64),

    #[error("invalid quantity: {0}")]
    InvalidQuantity(u64),

    #[error("invalid side: {0}")]
    InvalidSide(String),

    #[error("order already cancelled: {0}")]
    AlreadyCancelled(u64),

    #[error("order already filled: {0}")]
    AlreadyFilled(u64),

    #[error("insufficient liquidity")]
    InsufficientLiquidity,
}

/// Result type alias for order operations.
pub type OrderResult<T> = Result<T, OrderError>;
