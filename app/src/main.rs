use mega_etl::{
    async_trait, params, Conn, Params, Pipe, Transformer, TransformerError, TransformerResult,
};
use mega_etl::{prelude::*, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use std::time::{Duration, SystemTime};

#[derive(Debug)]
struct EthPrice {
    price: Option<f64>,
    update_time: SystemTime,
}

impl EthPrice {
    pub fn new() -> Self {
        EthPrice {
            price: None,
            update_time: SystemTime::now(),
        }
    }
}

lazy_static::lazy_static! {
    static ref PRICE: Mutex<EthPrice> = Mutex::new(EthPrice::new());
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Transaction {
    from_address: String,
    to_address: String,
    value_usd: String,
    value_eth: String,
    gas: f32,
    confirmed: bool,
}

#[async_trait]
impl Transformer for Transaction {
    async fn transform_save(
        inbound_data: Vec<u8>,
        conn: Arc<Mutex<Conn>>,
    ) -> TransformerResult<()> {
        let s = std::str::from_utf8(&inbound_data)
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let transaction: Value =
            serde_json::from_str(s).map_err(|e| TransformerError::Custom(e.to_string()))?;
        let status = serde_json::from_value::<String>(transaction["status"].clone())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let from_address = serde_json::from_value::<String>(transaction["from"].clone())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let to_address = serde_json::from_value::<String>(transaction["to"].clone())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let value_eth = serde_json::from_value::<String>(transaction["value"].clone())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        // get value in usd
        let now = SystemTime::now();
        let mut price = PRICE.lock().await;
        let current_price = if price.price.is_none()
            || now
                .duration_since(price.update_time)
                .map_err(|e| TransformerError::Custom(e.to_string()))?
                > Duration::from_secs(600)
        {
            log::debug!("try to get eth price");
            let mut buf = Vec::new(); //container for body of a response
            let api_key =
                std::env::var("TI_API_KEY").map_err(|e| TransformerError::Custom(e.to_string()))?;
            let res = http_req::request::get(
                format!(
                    "https://api.etherscan.io/api?module=stats&action=ethprice&apikey={}",
                    api_key
                ),
                &mut buf,
            )
            .unwrap();

            if !res.status_code().is_success() {
                return Err(TransformerError::Custom(format!(
                    "Request error: {}, get code: {}",
                    res.reason(),
                    res.status_code()
                )));
            }
            let response =
                std::str::from_utf8(&buf).map_err(|e| TransformerError::Custom(e.to_string()))?;
            let response: Value = serde_json::from_str(response)
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            serde_json::from_value::<String>(response["result"]["ethusd"].clone())
                .map_err(|e| TransformerError::Custom(e.to_string()))?
                .parse::<f64>()
                .map_err(|e| TransformerError::Custom(e.to_string()))?
        } else {
            log::debug!("use cached price");
            price.price.unwrap()
        };
        price.update_time = now;
        price.price = Some(current_price);
        // release the lock
        drop(price);
        let value_usd = value_eth
            .clone()
            .parse::<f64>()
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let value_usd = (value_usd / 1_000_000_000_000_000_000.0 * current_price).to_string();
        let gas = serde_json::from_value::<f32>(transaction["gas"].clone())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let tx = Transaction {
            from_address,
            to_address,
            value_eth,
            value_usd,
            gas,
            confirmed: status == "confirmed",
        };
        log::info!("{:?}", tx);
        let mut conn = conn.lock().await;
        // check if table exist:
        let result = conn
            .exec::<String, &str, ()>(r"SHOW TABLES LIKE 'transactions';", ())
            .await
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        if result.len() == 0 {
            // table doesn't exist, create a new one
            conn.exec::<String, &str, ()>(r"CREATE TABLE transactions (from_address VARCHAR(50), to_address VARCHAR(50), value_usd VARCHAR(50), value_eth VARCHAR(50), gas FLOAT, confirmed BOOL);", ())
                    .await
                    .map_err(|e| TransformerError::Custom(e.to_string()))?;
            log::debug!("create new table");
        }
        log::debug!("before insert");
        if tx.confirmed {
            // first check if there exists the pending record
            let tx1 = tx.clone();
            let result = conn.exec::<Row, String, Params>(
                String::from(
                    r"SELECT * FROM transactions WHERE from_address = :from_address AND to_address = :to_address AND value_eth = :value_eth AND gas = :gas;"
                ),
                params! {
                    "from_address" => tx1.from_address,
                    "to_address" => tx1.to_address,
                    "value_eth" => tx1.value_eth,
                    "gas" => tx1.gas,
                },
            )
            .await
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
            if result.len() == 0 {
                log::debug!("Insert confirmed record");
                // just insert
                conn.exec::<String, String, Params>(
                    String::from(
                        r"INSERT INTO transactions (from_address, to_address, value_usd, value_eth, gas, confirmed)
                    VALUES (:from_address, :to_address, :value_usd, :value_eth, :gas, :confirmed)",
                    ),
                    params! {
                        "from_address" => tx.from_address,
                        "to_address" => tx.to_address,
                        "value_usd" => tx.value_usd,
                        "value_eth" => tx.value_eth,
                        "gas" => tx.gas,
                        "confirmed" => tx.confirmed
                    },
                )
                .await
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            } else {
                log::debug!("Update confirmed record");
                // update existing record
                conn.exec::<Row, String, Params>(
                    String::from(
                        r"UPDATE transactions SET confirmed = :confirmed
                        WHERE from_address = :from_address AND to_address = :to_address AND value_eth = :value_eth AND gas = :gas;",
                    ),
                    params! {
                        "from_address" => tx.from_address,
                        "to_address" => tx.to_address,
                        "value_eth" => tx.value_eth,
                        "gas" => tx.gas,
                        "confirmed" => tx.confirmed
                    },
                )
                .await
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            }
            log::debug!("insert successfully");
            Ok(())
        } else if status == "pending" {
            log::debug!("Insert pending record");

            conn.exec::<String, String, Params>(
                String::from(
                    r"INSERT INTO transactions (from_address, to_address, value_usd, value_eth, gas, confirmed)
                VALUES (:from_address, :to_address, :value_usd, :value_eth, :gas, :confirmed)",
                ),
                params! {
                    "from_address" => tx.from_address,
                    "to_address" => tx.to_address,
                    "value_usd" => tx.value_usd,
                    "value_eth" => tx.value_eth,
                    "gas" => tx.gas,
                    "confirmed" => tx.confirmed
                },
            )
            .await
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
            log::debug!("insert successfully");
            Ok(())
        } else {
            log::debug!("Skip other records");

            // failing cases
            log::debug!("skip");
            Err(TransformerError::Skip)
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // can use builder later
    let uri = std::env::var("DATABASE_URL")?;
    let mut pipe = Pipe::new(uri, "http://0.0.0.0:3344".to_string()).await;

    pipe.start::<Transaction>().await?;
    Ok(())
}
