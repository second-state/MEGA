pub use async_trait::async_trait;
pub use mysql_async::prelude::*;
pub use mysql_async::*;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use url::Url;

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

impl DataSource {
    pub fn parse_uri(uri: &str) -> std::result::Result<Self, url::ParseError> {
        let url = Url::parse(uri)?;
        match url.scheme() {
            "http" => {
                let host = if let Some(host) = url.host_str() {
                    host
                } else {
                    return Err(url::ParseError::EmptyHost);
                };
                let port = if let Some(port) = url.port_or_known_default() {
                    port
                } else {
                    return Err(url::ParseError::InvalidPort);
                };
                Ok(DataSource::Hyper(host.to_string(), port))
            }
            "redis" => Ok(DataSource::Redis),
            "kfaka" => Ok(DataSource::Kafka),
            _ => Ok(DataSource::Unknown),
        }
    }
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
        let opts = Opts::from_url(mysql_uri.as_ref()).unwrap();
        let builder = OptsBuilder::from_opts(opts);
        let pool_opts = PoolOpts::default();
        let pool = Pool::new(builder.pool_opts(pool_opts));
        return Pipe {
            mysql_conn: Arc::new(pool),
            connector_uri: Some(data_source_uri),
        };
    }

    pub async fn start<T: Transformer + 'static>(&mut self) -> TransformerResult<()> {
        let uri = self.connector_uri.as_ref().unwrap();
        match DataSource::parse_uri(uri).map_err(|e| TransformerError::Custom(e.to_string()))? {
            DataSource::Hyper(addr, port) => Err(TransformerError::Unimplemented),
            DataSource::Redis | DataSource::Kafka => Err(TransformerError::Unimplemented),
            DataSource::Unknown => Err(TransformerError::Custom("Unknown data source".to_string())),
        }
    }
}
