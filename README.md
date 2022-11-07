# A serverless ETL runtime for cloud databases

MEGA stands for **Make ETLs Great Again!** [Checkout a video demo!](https://www.youtube.com/watch?v=sQCRQeTnBgo)

This project is a cloud-native ETL (Extract, Transform, Load) application framework based on the [WasmEdge](https://github.com/WasmEdge) WebAssembly runtime for developers to filter, map, and transform data pipelines going into cloud databases. We are currently targetting any MySQL compatible database as the backend.

ETL tools are crucial for the modern data analytics pipeline. However, ETL for cloud databases has its own challenges. Since the public cloud is fundamentally a multi-tenancy environment, all user-defined ETL functions are isolated outside of the database in separate VMs or secure containers. That is a complex and heavyweight setup, which is not suited for simple functions that need to process sporadic streams of data. 

With the MEGA framework, developers will be able to create secure, lightweight, fast and cross-platform ETL functions that are located close to or even embedded in cloud databases' infrastructure. The MEGA ETL functions can be deployed as serverless functions and receive data from a variety of sources including event queues, webhook callbacks and data streaming pipelines. The outcomes are written into the designated cloud database for later analysis.

## Examples

* [examples/order](examples/order) is an example to take orders from an e-commerce application via a HTTP webhook, and store the orders into a database. It is the example we will go through in this document. There is also [an alternative implementation](examples/order_conn) for this example -- it uses a direct connection to the backend database for more flexibility.
* [examples/kafka](examples/kafka) is an example to take those e-commerce orders from a Kafka / Redpanda queue, and store the orders into a database.
* [examples/ethereum](examples/ethereum) is an example to filter, transform, and store Ethereum transactions in a relational database.

## Prerequisites

The [WasmEdge](https://github.com/WasmEdge) WebAssembly Runtime is an open source project under the CNCF. It provides a safer and lighter alternative than Linux containers to run compiled (i.e., high-performance) ETL functions. They can be deployed to the edge cloud close to the data source or even colocate with the cloud database servers in the same firewall. Specially, you will need

* [Install Rust](https://www.rust-lang.org/tools/install). The framework is currently written in the Rust language. A JavaScript version is in the works.
* [Install WasmEdge](https://wasmedge.org/book/en/quick_start/install.html). You need it to run the ETL functions.
* Install a MySQL compatible analytical database or sign up for a cloud database. We recommend [TiDB Cloud](https://tidbcloud.com/). The ETL transformed data is written into this database for later analysis.

On Linux, you can use the following commands to install Rust and WasmEdge.

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
# Install WebAssembly target for Rust
rustup target add wasm32-wasi

# Install WasmEdge
curl -sSf https://raw.githubusercontent.com/WasmEdge/WasmEdge/master/utils/install.sh | bash -s -- -e all
source $HOME/.wasmedge/env
```

## Create the ETL function

First, add the MEGA crate to your Rust project.

```toml
[dependencies]
mega_etl = "0.1"
```

Next, in your Rust code, you will need to implement the following.

* Define a struct that models database table. Each column in the table is represented by a data field in the `struct`.
* Implement a required `transform()` function to give the above struct the `Transformer` trait. The function takes a `Vec<u8>` byte array as input argument, and returns a SQL string for the database.
* Set variables for the connection string to TiDB and configurations for the inbound connector where the input `Vec<u8>` would be retrieved (eg from a Kafka queue or a HTTP service or a temp database table in Redis).

First, let's define the data structure for the database table. It is a table for order records for an e-commerce web site.

```rust
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
```

Next, define the ETL `transform()` function that transforms inbound data into a set of SQL statements for the database. The inbound data is simply a byte array that is recived from any data source (e.g., a POST request on the web hook, or a message in Kafka). In this example, the inbound data is a JSON string that represents the `order`.

```rust
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
            r"INSERT INTO orders VALUES ({:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, CURRENT_TIMESTAMP);",
            order.order_id,
            order.product_id,
            order.quantity,
            order.amount,
            order.shipping,
            order.tax,
            order.shipping_address,
        );
        dbg!(sql_string.clone());
        ret.push(sql_string);
        Ok(ret)
    }
}
```

Finally, in the main application we will configure an outbound database (a cloud database instance specified in `DATABASE_URL`) and an inbound data source (a webhook at `http://my.ip:3344`). Other inbound methods are also supported. For example, you can configure the ETL function to receive messages from a [Kafka or Redpanda queue](examples/kafka) or a Redis table.

```bash
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let uri = std::env::var("DATABASE_URL")?;
    let mut pipe = Pipe::new(uri, "http://0.0.0.0:3344".to_string()).await;

    // This is async because this calls the async transform() function in Order
    pipe.start::<Order>().await?;
    Ok(())
}
```

Optionally, you can define an `init()` function. It will be executed the first time when the ETL starts up. Here, we use the `init()` to create and empty `orders` table in the database.

```rust
#[async_trait]
impl Transformer for Order {
    async fn init() -> TransformerResult<String> {
        Ok(String::from(
            r"CREATE TABLE IF NOT EXISTS orders (order_id INT, product_id INT, quantity INT, amount FLOAT, shipping FLOAT, tax FLOAT, shipping_address VARCHAR(50), date_registered TIMESTAMP DEFAULT CURRENT_TIMESTAMP);",
        ))
    }
}
```

## Build

Use the Rust `cargo` tool to build the ETL application.

```bash
cargo build --target wasm32-wasi --release
```

Optionally, you could AOT compile it to improve performance (could be 100x faster for compute-intensive ETL functions).

```bash
wasmedgec target/wasm32-wasi/release/order.wasm order.wasm
```

## Run

With WasmEdge, you have many deployment options. You could run the compiled ETL function program in any serverless infra that supports WasmEdge, which includes almost all [Kubernetes variations](https://wasmedge.org/book/en/use_cases/kubernetes.html), [Dapr](https://github.com/second-state/dapr-wasm), Docker, [Podman](https://github.com/KWasm/podman-wasm) and hosted function schedulers such as [essa-rs](https://github.com/essa-project/essa-rs) and [flows.network](https://flows.network/).

But in this example, we will just use the good old `wasmedge` CLI tool to run the ETL function-as-a-service.

```bash
wasmedge --env DATABASE_URL=mysql://user:pass@ip.address:3306/mysql order.wasm
```

It starts an HTTP server on port 3344 and waits for the inbound data. Open another terminal, and send it some inbound data via `curl`.

```bash
curl http://localhost:3344/ -X POST -d @order.json
```

The JSON data in `order.json` is sent to the ETL `transform()` function as inbound data. The function parses it and generates the SQL string, which is automatically executed on the connected TiDB Cloud instance. You can now connect to TiDB Cloud from your database browser and see the `order` record in the database.

### Resources

* [WasmEdge Runtime](https://github.com/WasmEdge/)
* [WasmEdge book](https://wasmedge.org/book/en/)
* [Second State](https://www.secondstate.io/)
* [TiDB project](https://github.com/pingcap/tidb)
* [TiDB Cloud](https://tidbcloud.com/)

### Join us!

* [Twitter @realwasmedge](https://twitter.com/realwasmedge)
* [Discord](https://discord.gg/JHxMj9EQbA)
* [Email list](https://groups.google.com/g/wasmedge/)
* [Slack #WasmEdge](https://slack.cncf.io/)

