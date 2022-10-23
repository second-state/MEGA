# Blockchain transactions to relational database table mapping (TRM)

## Prerequisites

* [Install Rust](https://www.rust-lang.org/tools/install). The framework is currently written in the Rust language. A JavaScript version is in the works.
* [Install WasmEdge](https://wasmedge.org/book/en/quick_start/install.html). You need it to run the ETL functions.
* [Sign up for TiDB Cloud](https://tidbcloud.com/). The ETL transformed transaction data is written into this database for later analysis.
* [Sign up for mempool API](https://www.blocknative.com/api). This service will call your webhook when a new Ethereum transaction enters the mempool or is confirmed on the blockchain.
* [Sign up for Etherscan API](https://etherscan.io/apis). You need this to look up ETH to USD exchange rate in real time to convert input ETH values into USD and then save to TiDB.

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

## Build

Use the following command to build the microservice. A WebAssembly bytecode file (`wasm` file) will be created.

```bash
cargo build --target wasm32-wasi --release
```

You can run the AOT compiler on the `wasm` file. It could significantly improvement the performance of compute-intensive applications. This microservice, however, is a network intensitive application. Our use of async HTTP networking (Tokio and hyper) and async MySQL connectors are crucial for the performance of this microservice.

```bash
wasmedgec target/wasm32-wasi/release/transaction.wasm transaction.wasm
```

## Run

You can use the `wasmedge` command to run the `wasm` application. It will start the server. Make sure that you pass the following env variables to the command. 

* The `DATABASE_URL` is the [TiDB Cloud instance URL you signed up for](https://tidbcloud.com/). Make sure that you replace the `user` and `pass` with your own.
* The `PRICE_API_KEY` is the [Etherscan ETH/USD price lookup service you signed up for](https://etherscan.io/apis).

```bash
nohup wasmedge --env DATABASE_URL=mysql://user:pass@gateway01.us-west-2.prod.aws.tidbcloud.com:4000/demo --env PRICE_API_KEY=ABCD1234 --env RUST_LOG=info transaction.wasm 2>&1 &
```

Once the server is running and listening at the `3344` port, go to the [Blocknative mempool API console you signed up for](https://www.blocknative.com/api), and enter the ETL function's URL (e.g., `http://my-aws-ec2.ip;3344/`) as the webhook URL.

## See it in action

The `nohup.out` file contains the raw transactions the ETL received from the mempool service. Use the following command to see live updates on the server.

```bash
tail -f nohup.out
```

In the TiDB Cloud console, go to "Connect -> Web SQL Shell", you can execute SQL statements to see the content in the database table.

```sql
USE demo;
SELECT * from transactions;
```
