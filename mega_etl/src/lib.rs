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

pub struct Pipe {
    mysql_conn: Arc<Pool>,
    connector_uri: Option<String>,
}

impl Pipe {
    pub async fn new<Str: AsRef<str>>(mysql_uri: Str, data_source_uri: String) -> Self {
        todo!()
    }

    pub async fn start<T: Transformer + 'static>(&mut self) -> TransformerResult<()> {
        todo!()
    }
}
