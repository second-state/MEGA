pub use async_trait::async_trait;
use futures_util::StreamExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use hyper::{Body, Request, Response};
pub use mysql_async::prelude::*;
pub use mysql_async::*;
use rskafka::client::{
    consumer::{StartOffset, StreamConsumerBuilder},
    partition::UnknownTopicHandling,
    ClientBuilder,
};
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

macro_rules! impl_from_transformer_error {
    ($error:ty) => {
        impl From<$error> for TransformerError {
            fn from(value: $error) -> Self {
                TransformerError::Custom(value.to_string())
            }
        }
    };
}

impl_from_transformer_error!(rskafka::client::error::Error);
impl_from_transformer_error!(mysql_async::Error);
impl_from_transformer_error!(mysql_async::ParseError);
impl_from_transformer_error!(std::io::Error);
impl_from_transformer_error!(hyper::Error);

enum DataSource {
    Hyper(String, u16),
    Redis(String, String),
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
            //"redis" => Ok(DataSource::Redis(Host,Port)),
            "redis" => {
                let host= if let Some(host) = url.host_str(){
                    host
                }
                else{
                    return Err(url::ParseError::EmptyHost);
                };
                let password = if let Some(password) = url.password(){
                    password
                }
                else{
                    "Incorrect Password Provided!"
                };
                Ok(DataSource::Redis(host.to_string(), password.to_string()))
            }
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
                let mut conn = self.mysql_conn.get_conn().await?;
                let _ = conn.exec::<String, String, ()>(sql_string, ()).await?;
            }
            Err(e) => return Err(e),
        }
        let uri = self.connector_uri.as_ref().unwrap();
        match DataSource::parse_uri(uri)? {
            DataSource::Hyper(addr, port) => {
                let addr = (addr, port).to_socket_addrs()?.nth(0);
                let addr = if addr.is_none() {
                    return Err(TransformerError::Custom("Empty addr".into()));
                } else {
                    addr.unwrap()
                };

                let make_svc = make_service_fn(|_| {
                    let pool = self.mysql_conn.clone();
                    async move {
                        let conn = Arc::new(Mutex::new(pool.get_conn().await.unwrap()));
                        Ok::<_, Infallible>(service_fn(move |req| {
                            let conn = conn.clone();
                            handle_request::<T>(req, conn)
                        }))
                    }
                });
                let server = Server::bind(&addr).serve(make_svc);
                server.await?;
                Ok(())
            }
            DataSource::Kafka(host, port, topic) => {
                log::debug!("{} {} {}", host, port, topic);
                let connection = format!("{}:{}", host, port);
                let timeout = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    ClientBuilder::new(vec![connection]).build(),
                )
                .await;
                let client = if let Ok(client) = timeout {
                    client?
                } else {
                    panic!("Cannot connect Kafka in 3s.");
                };

                let partition_client = Arc::new(
                    client
                        .partition_client(
                            topic,
                            0, // partition
                            UnknownTopicHandling::Retry,
                        )
                        .await?,
                );
                let mut stream =
                    StreamConsumerBuilder::new(Arc::clone(&partition_client), StartOffset::Latest)
                        .with_max_wait_ms(500)
                        .build();
                // use loop to listen incoming records.
                loop {
                    log::debug!("wait a record");
                    let (mut record, _high_watermark) = stream
                        .next()
                        .await
                        .ok_or(TransformerError::Custom("kafka stream return error".into()))??;
                    if let Some(incoming_data) = record.record.value.take() {
                        log::debug!("get a record");
                        let conn = Arc::new(Mutex::new(self.mysql_conn.get_conn().await?));
                        kafka_handle_request::<T>(incoming_data, conn).await?;
                    } else {
                        log::debug!("skip empty kafka record");
                    }
                }
            }
            DataSource::Redis(host,password) => {
                let redis_hostname = host;
                let redis_password = password;
                let redis_conn_url = format!("redis://:{}@{}", redis_password, redis_hostname);
                redis::Client::open(redis_conn_url).expect("Invalid Connection URL").get_connection().expect("Failed to connect to Redis");
                Ok(())
            }
            DataSource::Unknown => Err(TransformerError::Custom("Unknown data source".to_string())),
        }
    }
}
