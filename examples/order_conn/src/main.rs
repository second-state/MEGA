use mega_etl::prelude::*;
use mega_etl::{
    async_trait, params, Conn, Params, Pipe, Transformer, TransformerError, TransformerResult,
};

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Debug)]
struct Order {
    order_id: i32,
    product_id: i32,
    quantity: i32,
    amount: f32,
    shipping: f32,
    tax: f32,
    shipping_address: String,
}

#[async_trait]
impl Transformer for Order {
    async fn transform_save(
        inbound_data: &Vec<u8>,
        conn: Arc<Mutex<Conn>>,
    ) -> TransformerResult<()> {
        let s = std::str::from_utf8(&inbound_data)
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let order: Order = serde_json::from_str(String::from(s).as_str())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        log::info!("{:?}", &order);
        let mut conn = conn.lock().await;
        let result = conn
            .exec::<String, &str, ()>(r"SHOW TABLES LIKE 'orders';", ())
            .await
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        if result.len() == 0 {
            // table doesn't exist, create a new one
            conn.exec::<String, &str, ()>(r"CREATE TABLE orders (order_id INT, product_id INT, quantity INT, amount FLOAT, shipping FLOAT, tax FLOAT, shipping_address VARCHAR(50), date_registered TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP );", ())
                .await
                .map_err(|e| TransformerError::Custom(e.to_string()))?;
            log::debug!("create new table");
        }
        log::debug!("before insert");
        conn.exec::<String, String, Params>(String::from(r"INSERT INTO orders (order_id, product_id, quantity, amount, shipping, tax, shipping_address)
        VALUES (:order_id, :product_id, :quantity, :amount, :shipping, :tax, :shipping_address)"), params! {
            "order_id" => order.order_id,
            "product_id" => order.product_id,
            "quantity" => order.quantity,
            "amount" => order.amount,
            "shipping" => order.shipping,
            "tax" => order.tax,
            "shipping_address" => &order.shipping_address,
        }).await.map_err(|e| TransformerError::Custom(e.to_string()))?;
        log::debug!("insert successfully");

        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // can use builder later
    let uri = std::env::var("DATABASE_URL")?;
    let mut pipe = Pipe::new(uri, "http://0.0.0.0:3344".to_string()).await;

    // This is async because this calls the async transform() function in Order
    pipe.start::<Order>().await?;
    Ok(())
}
