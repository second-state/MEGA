# WasmEdge ETL function example

## Prerequisites

* [Install Rust](https://www.rust-lang.org/tools/install). The framework is currently written in the Rust language. A JavaScript version is in the works.
* [Install WasmEdge](https://wasmedge.org/book/en/quick_start/install.html). You need it to run the ETL functions.
* [Install and start a MySQL compatible database](https://dev.mysql.com/doc/mysql-installation-excerpt/8.0/en/). The ETL transformed transaction data is written into this database for later analysis.

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

You can run the AOT compiler on the `wasm` file. It could significantly improvement the performance.

```bash
wasmedgec target/wasm32-wasi/release/order.wasm order.wasm
```

## Run

You can use the `wasmedge` command to run the `wasm` application. It will start the server. Make sure that you pass the `DATABASE_URL` that points to your running MySQL server.

```bash
nohup wasmedge --env "DATABASE_URL=mysql://user:pass@ip.address:3306/mysql" order.wasm 2>&1 &
```

The server log will appear in the `nohup.out` file.

## See it in action

You can send data to the webhook using `curl`.

```bash
curl http://localhost:3344/ -X POST -d @order.json
```

You can now log into the database to see the `orders` table and its content.

## Docker

With WasmEdge support in Docker, you can also use Docker Compose to build and start this multi-container application in a single command without installing any dependencies.

* [Install Docker Desktop + Wasm (Beta)](https://docs.docker.com/desktop/wasm/)
* or [Install Docker CLI + Wasm](https://github.com/chris-crone/wasm-day-na-22/tree/main/server)

Then, you just need to type one command.

```bash
docker compose up
```

This will build the Rust source code, run the Wasm server for the ETL function, and startup a MySQL backing database. You can then use [the curl commands](#see-it-in-action) to send data to the webhook for the ETL.

To see the data in the database container, you can use the following commands.

```bash
docker compose exec db /bin/bash 
root@c97c472db02e:/# mysql -u root -pwhalehello mysql
mysql> select * from orders;
... ...
```
