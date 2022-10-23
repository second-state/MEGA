use mega_etl::{async_trait, Pipe, Transformer, TransformerError, TransformerResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    value_usd: f64,
    value_eth: u64,
    gas: u64,
    confirmed: bool,
}

#[async_trait]
impl Transformer for Transaction {
    async fn transform(inbound_data: Vec<u8>) -> TransformerResult<Vec<String>> {
        log::info!("Receive data.");
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
        if value_eth == "0" {
            // ignore smart contracts
            log::info!("Skip smart contract");
            return Err(TransformerError::Skip);
        }
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
            let api_key = std::env::var("PRICE_API_KEY")
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
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
        let value_eth = value_eth
            .parse::<u64>()
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let value_usd = value_eth as f64 / 1_000_000_000_000_000_000.0 * current_price;
        let gas = serde_json::from_value::<u64>(transaction["gas"].clone())
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
        log::debug!("before insert");
        if tx.confirmed {
            let sql_string = format!(
                r"INSERT INTO transactions (from_address, to_address, value_usd, value_eth, gas, confirmed) VALUES({:?}, {:?}, {:?}, {:?}, {:?}, {:?}) ON DUPLICATE KEY UPDATE confirmed=1;",
                tx.from_address, tx.to_address, tx.value_usd, tx.value_eth, tx.gas, tx.confirmed
            );
            log::debug!("insert successfully");
            Ok(vec![sql_string])
        } else if status == "pending" {
            log::debug!("Insert pending record");
            let sql_string = format!(
                r"INSERT INTO transactions (from_address, to_address, value_usd, value_eth, gas, confirmed) VALUES({:?}, {:?}, {:?}, {:?}, {:?}, {:?});",
                tx.from_address, tx.to_address, tx.value_usd, tx.value_eth, tx.gas, tx.confirmed
            );
            log::debug!("insert successfully");
            Ok(vec![sql_string])
        } else {
            log::debug!("Skip other records");
            // failing cases
            log::info!("Skip other records");
            Err(TransformerError::Skip)
        }
    }

    async fn init() -> TransformerResult<String> {
        Ok(String::from(
            r"CREATE TABLE transactions (from_address VARCHAR(50), to_address VARCHAR(50), value_usd FLOAT, value_eth BIGINT UNSIGNED, gas BIGINT UNSIGNED, confirmed BOOL, date_registered TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP);",
        ))
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
