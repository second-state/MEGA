pub use async_trait::async_trait;
pub use mysql_async::prelude::*;
pub use mysql_async::*;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransformerError {
    #[error("function is unimplemented")]
    Unimplemented,
    #[error("Error: {0}")]
    Custom(String),
    #[error("unknown data store error")]
    Unknown,
}

pub type TransformerResult<T> = std::result::Result<T, TransformerError>;

enum DataSource {
    Hyper(String, u16),
    Redis,
    Kafka,
    Unknown,
}

#[async_trait]
pub trait Transformer {
    async fn transform(_inbound_data: Vec<u8>) -> TransformerResult<String> {
        Err(TransformerError::Unimplemented.into())
    }

    async fn transform_save(
        _inbound_data: Vec<u8>,
        _conn: Arc<Mutex<Conn>>,
    ) -> TransformerResult<()> {
        Err(TransformerError::Unimplemented.into())
    }
}
