use mega_etl::prelude::*;
use mega_etl::{
    async_trait, params, Conn, Params, Pipe, Transformer, TransformerError, TransformerResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Debug)]
struct Transaction {
    from_address: String,
    to_address: String,
    value_usd: String,
    value_eth: String,
    gas: f32,
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
        if status == "confirmed" {
            let from_address = serde_json::from_value::<String>(transaction["from"].clone())
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            let to_address = serde_json::from_value::<String>(transaction["to"].clone())
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            let value_eth = serde_json::from_value::<String>(transaction["value"].clone())
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            // get value in usd
            // let mut buf = Vec::new(); //container for body of a response
            // let uri: Uri =
            //     Uri::try_from("https://api.tokeninsight.com/api/v1/simple/price?ids=ethereum")
            //         .unwrap();
            // let api_key =
            //     std::env::var("TI_API_KEY").map_err(|e| TransformerError::Custom(e.to_string()))?;
            // let res = Request::new(&uri)
            //     .header("accept", "application/json")
            //     .header("TI_API_KEY", &api_key)
            //     .send(&mut buf)
            //     .map_err(|e| TransformerError::Custom(e.to_string()))?;
            // if !res.status_code().is_success() {
            //     return Err(TransformerError::Custom(format!(
            //         "Request error: {}, get code: {}",
            //         res.reason(),
            //         res.status_code()
            //     )));
            // }
            // let response =
            //     std::str::from_utf8(&buf).map_err(|e| TransformerError::Custom(e.to_string()))?;
            // let response: Value = serde_json::from_str(response)
            //     .map_err(|e| TransformerError::Custom(e.to_string()))?;
            // let current_price = serde_json::from_value::<f64>(
            //     response["data"][0]["price"][0]["price_latest"].clone(),
            // )
            // .map_err(|e| TransformerError::Custom(e.to_string()))?;
            let current_price = 1283.6475;

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
            };
            dbg!(&tx);
            let mut conn = conn.lock().await;
            // check if table exist:
            let result = conn
                .exec::<String, &str, ()>(r"SHOW TABLES LIKE 'transactions';", ())
                .await
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            if result.len() == 0 {
                // table doesn't exist, create a new one
                conn.exec::<String, &str, ()>(r"CREATE TABLE transactions (from_address VARCHAR(50), to_address VARCHAR(50), value_usd VARCHAR(50), value_eth VARCHAR(50), gas FLOAT);", ())
                    .await
                    .map_err(|e| TransformerError::Custom(e.to_string()))?;
                log::info!("create new table");
            }
            log::info!("before insert");
            conn.exec::<String, String, Params>(
                String::from(
                    r"INSERT INTO transactions (from_address, to_address, value_usd, value_eth, gas)
            VALUES (:from_address, :to_address, :value_usd, :value_eth, :gas)",
                ),
                params! {
                    "from_address" => tx.from_address,
                    "to_address" => tx.to_address,
                    "value_usd" => tx.value_usd,
                    "value_eth" => tx.value_eth,
                    "gas" => tx.gas,
                },
            )
            .await
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
            log::info!("insert successfully");
            Ok(())
        } else {
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
