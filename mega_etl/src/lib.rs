pub use async_trait::async_trait;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use hyper::{Body, Request, Response};
pub use mysql_async::prelude::*;
pub use mysql_async::*;
use std::convert::Infallible;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use url::Url;

#[derive(Error, Debug)]
pub enum TransformerError {
    #[error("function is unimplemented")]
    Unimplemented,
    #[error("Error: {0}")]
    Custom(String),
    #[error("unknown data store error")]
    Unknown,
    #[error("skip the data")]
    Skip,
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
    async fn transform(_inbound_data: Vec<u8>) -> TransformerResult<Vec<String>> {
        Err(TransformerError::Unimplemented.into())
    }

    async fn transform_save(
        _inbound_data: Vec<u8>,
        _conn: Arc<Mutex<Conn>>,
    ) -> TransformerResult<()> {
        Err(TransformerError::Unimplemented.into())
    }

    async fn init() -> TransformerResult<String> {
        Err(TransformerError::Unimplemented.into())
    }
}

async fn handle_request<T: Transformer>(
    req: Request<Body>,
    conn: Arc<Mutex<Conn>>,
) -> anyhow::Result<Response<Body>> {
    log::debug!("receive data");
    let content = hyper::body::to_bytes(req.into_body())
        .await
        .unwrap()
        .to_vec();

    match T::transform(content.clone()).await {
        Ok(sql_strings) => {
            // exec the query string
            // conn.lock().await.exec(stmt, params)
            for sql_string in sql_strings {
                log::debug!("receive {sql_string:?}");

                if let Err(e) = conn
                    .lock()
                    .await
                    .exec::<String, String, ()>(sql_string, ())
                    .await
                {
                    return Ok(Response::new(Body::from(e.to_string())));
                }
            }
            return Ok(Response::new(Body::from("Success")));
        }
        Err(TransformerError::Unimplemented) => {
            log::debug!("skip transform");
        }
        Err(TransformerError::Custom(err)) => return Ok(Response::new(Body::from(err))),
        Err(TransformerError::Unknown) => return Ok(Response::new(Body::from("Unknown error"))),
        Err(TransformerError::Skip) => return Ok(Response::new(Body::from("skip this data"))),
    }
    match T::transform_save(content, conn).await {
        Ok(_) => {
            // exec the query string
            // conn.lock().await.exec(stmt, params)
            return Ok(Response::new(Body::from("Success")));
        }
        Err(TransformerError::Unimplemented) => {
            log::debug!("skip transform_save");
        }
        Err(TransformerError::Custom(err)) => return Ok(Response::new(Body::from(err))),
        Err(TransformerError::Unknown) => return Ok(Response::new(Body::from("Unknown error"))),
        Err(TransformerError::Skip) => return Ok(Response::new(Body::from("skip this data"))),
    }
    // T::transform(content, conn).await;
    Ok(Response::new(Body::from(
        "One of transform and transform_save must be implemented.",
    )))
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
        // init the table
        match T::init().await {
            Ok(sql_string) => {
                let mut conn = self
                    .mysql_conn
                    .get_conn()
                    .await
                    .map_err(|e| TransformerError::Custom(e.to_string()))?;
                let _ = conn
                    .exec::<String, String, ()>(sql_string, ())
                    .await
                    .map_err(|e| TransformerError::Custom(e.to_string()))?;
            }
            Err(e) => return Err(e),
        }
        let uri = self.connector_uri.as_ref().unwrap();
        match DataSource::parse_uri(uri).map_err(|e| TransformerError::Custom(e.to_string()))? {
            DataSource::Hyper(addr, port) => {
                let addr = (addr, port)
                    .to_socket_addrs()
                    .map_err(|e| TransformerError::Custom(e.to_string()))?
                    .nth(0);
                let addr = if addr.is_none() {
                    return Err(TransformerError::Custom("Empty addr".into()));
                } else {
                    addr.unwrap()
                };

                let make_svc = make_service_fn(|_| {
                    let pool = self.mysql_conn.clone();
                    async move {
                        let conn = Arc::new(Mutex::new(
                            pool.get_conn()
                                .await
                                .map_err(|e| TransformerError::Custom(e.to_string()))
                                .unwrap(),
                        ));
                        Ok::<_, Infallible>(service_fn(move |req| {
                            let conn = conn.clone();
                            handle_request::<T>(req, conn)
                        }))
                    }
                });
                let server = Server::bind(&addr).serve(make_svc);
                server
                    .await
                    .map_err(|e| TransformerError::Custom(e.to_string()))?;
                Ok(())
            }
            DataSource::Redis | DataSource::Kafka => Err(TransformerError::Unimplemented),
            DataSource::Unknown => Err(TransformerError::Custom("Unknown data source".to_string())),
        }
    }
}
