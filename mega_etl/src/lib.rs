pub use async_trait::async_trait;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use hyper::{Body, Request, Response};
pub use mysql_async::prelude::*;
pub use mysql_async::*;
use rskafka::client::{partition::UnknownTopicHandling, ClientBuilder};
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

fn to_transformer_error(error: impl std::error::Error) -> TransformerError {
    TransformerError::Custom(error.to_string())
}

enum DataSource {
    Hyper(String, u16),
    Redis,
    Kafka(String, u16, String),
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
            "kafka" => {
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
                let topic = url
                    .path_segments()
                    .expect("rskafka: get invaild path")
                    .nth(0)
                    .expect("rskafka: get empty path");
                Ok(DataSource::Kafka(host.to_string(), port, topic.to_string()))
            }
            _ => Ok(DataSource::Unknown),
        }
    }
}

#[async_trait]
pub trait Transformer {
    async fn transform(_inbound_data: &Vec<u8>) -> TransformerResult<Vec<String>> {
        Err(TransformerError::Unimplemented.into())
    }

    async fn transform_save(
        _inbound_data: &Vec<u8>,
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

    match T::transform(&content).await {
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
    match T::transform_save(&content, conn).await {
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
    Ok(Response::new(Body::from(
        "One of transform and transform_save must be implemented.",
    )))
}

async fn kafka_handle_request<T: Transformer>(
    content: Vec<u8>,
    conn: Arc<Mutex<Conn>>,
) -> TransformerResult<()> {
    log::debug!("receive data");
    match T::transform(&content).await {
        Ok(sql_strings) => {
            // exec the query string
            // conn.lock().await.exec(stmt, params)
            for sql_string in sql_strings {
                log::debug!("receive {sql_string:?}");

                if let Err(_e) = conn
                    .lock()
                    .await
                    .exec::<String, String, ()>(sql_string, ())
                    .await
                {
                    log::error!("{:?}", _e.to_string());
                    return Ok(());
                }
            }
            return Ok(());
        }
        Err(TransformerError::Unimplemented) => {
            log::debug!("skip transform");
        }
        Err(TransformerError::Custom(_err)) => {
            log::error!("{:?}", _err.to_string());
            return Ok(());
        }
        Err(TransformerError::Unknown) => return Ok(()),
        Err(TransformerError::Skip) => return Ok(()),
    }
    match T::transform_save(&content, conn).await {
        Ok(_) => {
            // exec the query string
            // conn.lock().await.exec(stmt, params)
            return Ok(());
        }
        Err(TransformerError::Unimplemented) => {
            log::debug!("skip transform_save");
        }
        Err(TransformerError::Custom(_err)) => {
            log::error!("{:?}", _err.to_string());
            return Ok(());
        }
        Err(TransformerError::Unknown) => return Ok(()),
        Err(TransformerError::Skip) => return Ok(()),
    }
    Ok(())
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
                    .map_err(to_transformer_error)?;
                let _ = conn
                    .exec::<String, String, ()>(sql_string, ())
                    .await
                    .map_err(to_transformer_error)?;
            }
            Err(e) => return Err(e),
        }
        let uri = self.connector_uri.as_ref().unwrap();
        match DataSource::parse_uri(uri).map_err(to_transformer_error)? {
            DataSource::Hyper(addr, port) => {
                let addr = (addr, port)
                    .to_socket_addrs()
                    .map_err(to_transformer_error)?
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
                            pool.get_conn().await.map_err(to_transformer_error).unwrap(),
                        ));
                        Ok::<_, Infallible>(service_fn(move |req| {
                            let conn = conn.clone();
                            handle_request::<T>(req, conn)
                        }))
                    }
                });
                let server = Server::bind(&addr).serve(make_svc);
                server.await.map_err(to_transformer_error)?;
                Ok(())
            }
            DataSource::Kafka(host, port, topic) => {
                log::debug!("{} {} {}", host, port, topic);
                let connection = format!("{}:{}", host, port);
                let client = ClientBuilder::new(vec![connection])
                    .build()
                    .await
                    .map_err(to_transformer_error)?;

                let partition_client = client
                    .partition_client(
                        topic,
                        0, // partition
                        UnknownTopicHandling::Retry,
                    )
                    .await
                    .map_err(to_transformer_error)?;

                let (mut records, _high_watermark) = partition_client
                    .fetch_records(
                        0,            // offset
                        1..1_000_000, // min..max bytes
                        1_000_000,    // max wait time
                    )
                    .await
                    .map_err(to_transformer_error)?;
                for record in records.iter_mut() {
                    if let Some(incoming_data) = record.record.value.take() {
                        let conn = Arc::new(Mutex::new(
                            self.mysql_conn
                                .get_conn()
                                .await
                                .map_err(to_transformer_error)?,
                        ));
                        kafka_handle_request::<T>(incoming_data, conn).await?;
                    }
                }

                Ok(())
            }
            DataSource::Redis => Err(TransformerError::Unimplemented),
            DataSource::Unknown => Err(TransformerError::Custom("Unknown data source".to_string())),
        }
    }
}
