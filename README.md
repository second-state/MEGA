# A serverless ETL runtime for cloud databases

MEGA stands for **Make ETLs Great Again!**

This project is a cloud-native ETL (Extract, Transform, Load) application framework based on the [WasmEdge](https://github.com/WasmEdge) WebAssembly runtime for developers to filter, map, and transform data pipelines going into cloud databases. We are currently targetting the [TiDB Cloud](https://tidbcloud.com/) as the backend database.

ETL tools are crucial for the modern data analytics pipeline. However, ETL for cloud databases has its own challenges. Since the public cloud is fundamentally a multi-tenancy environment, all user-defined ETL functions are isolated outside of the database in separate VMs or secure containers. That is a complex and heavyweight setup, which is not suited for simple functions that need to process sporadic streams of data. 

With the MEGA framework, developers will be able to create secure, lightweight, fast and cross-platform ETL functions that are located close to or even embedded in cloud databases' infrastructure. The MEGA ETL functions can be deployed as serverless functions and receive data from a variety of sources including event queues, webhook callbacks and data streaming pipelines. The outcomes are written into the designated cloud database for later analysis.

## Prerequisites

The [WasmEdge](https://github.com/WasmEdge) WebAssembly Runtime is an open source project under the CNCF. It provides a safer and lighter alternative than Linux containers to run compiled (i.e., high-performance) ETL functions. They can be deployed to the edge cloud close to the data source or even colocate with the cloud database servers in the same firewall. Specially, you will need

* [Install Rust](https://www.rust-lang.org/tools/install). The framework is currently written in the Rust language. A JavaScript version is in the works.
* [Install WasmEdge](https://wasmedge.org/book/en/quick_start/install.html). You need it to run the ETL functions.
* [Sign up for TiDB Cloud](https://tidbcloud.com/). The ETL transformed data is written into this database for later analysis.

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

Next, define the ETL `transform()` function that transforms inbound data into a SQL statement for the database. The inbound data is simply a byte array that is recived from any data source (e.g., a POST request on the web hook, or a message in Kafka). In this example, the inbound data is a JSON string that represents the `order`.

```rust
#[async_trait]
impl Transformer for Order {
    async fn transform(inbound_data: Vec<u8>) -> TransformerResult<String> {
        let s = std::str::from_utf8(&inbound_data)
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        let order: Order = serde_json::from_str(String::from(s).as_str())
            .map_err(|e| TransformerError::Custom(e.to_string()))?;
        dbg!(&order);
        
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

        Ok(sql_string)
    }
}
```

Finally, in the main application we will configure a inbound data source (a webhook at `http://localhost:8080`) and an outbound database (a TiDB Cloud instance).

```bash
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // can use builder later
    let uri = match std::env::var("DATABASE_URL") {
        Ok(uri) => uri,
        Err(_) => "mysql://userID:pass@gateway01.us-west-2.prod.aws.tidbcloud.com:4000/test".into(),
    };
    let mut pipe = Pipe::new(uri, "127.0.0.1:8080".to_string()).await;

    // This is async because this calls the async transform() function in Order
    pipe.start::<Order>().await?;
    Ok(())
}
```

Optionally, you can define an `init()` function. It will be executed the first time when the ETL starts up. Here, we use the `init()` to create and empty `orders` table in the database.

```rust
impl Transformer for Order {
    async fn init() -> TransformerResult<String> {
        let sql_string = "DROP TABLE IF EXISTS orders; CREATE TABLE orders (order_id INT, product_id INT, quantity INT, amount FLOAT, shipping FLOAT, tax FLOAT, shipping_address VARCHAR(20));";
        Ok(sql_string)
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
wasmedgec target/wasm32-wasi/release/order_demo.wasm order_demo.wasm
```

## Run

With WasmEdge, you have many deployment options. You could run the compiled ETL function program in any serverless infra that supports WasmEdge, which includes almost all [Kubernetes variations](https://wasmedge.org/book/en/use_cases/kubernetes.html), [Dapr](https://github.com/second-state/dapr-wasm), Docker, [Podman](https://github.com/KWasm/podman-wasm) and hosted function schedulers such as [essa-rs](https://github.com/essa-project/essa-rs) and [flows.network](https://flows.network/).

But in this example, we will just use the good old `wasmedge` CLI tool to run the ETL function-as-a-service.

```bash
wasmedge order_demo.wasm
```

It starts an HTTP server on port 8080 and waits for the inbound data. Open another terminal, and send it some inbound data via `curl`.

```bash
curl http://localhost:8080/ -X POST -d @order.json
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

