use mega_etl::{async_trait, Pipe, Transformer, TransformerError, TransformerResult};

use serde::{Deserialize, Serialize};
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
    async fn transform(inbound_data: Vec<u8>) -> TransformerResult<Vec<String>> {
        let s = std::str::from_utf8(&inbound_data)
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let order: Order = serde_json::from_str(String::from(s).as_str())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        log::info!("{:?}", &order);
        let mut ret = vec![];
        let sql_string = format!(
            r"INSERT INTO orders VALUES ({:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?});",
            order.order_id,
            order.product_id,
            order.quantity,
            order.amount,
            order.shipping,
            order.tax,
            order.shipping_address,
        );
        ret.push(sql_string);
        Ok(ret)
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
