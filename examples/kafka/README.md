# WasmEdge ETL function example for Redpanda / Kafka

## Prerequisites

* [Install Rust](https://www.rust-lang.org/tools/install). The framework is currently written in the Rust language. A JavaScript version is in the works.
* [Install WasmEdge](https://wasmedge.org/book/en/quick_start/install.html). You need it to run the ETL functions.
* [Install and start Redpanda](https://docs.redpanda.com/docs/platform/quickstart/quick-start-linux/). The ETL will transform data produced by a Redpanda queue.
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
wasmedgec target/wasm32-wasi/release/kafka.wasm kafka.wasm
```

## Run

You can use the `wasmedge` command to run the `wasm` application. It will start the server. Make sure that you pass the `DATABASE_URL` that points to your running MySQL server.

```bash
nohup wasmedge --env "DATABASE_URL=mysql://user:pass@ip.address:3306/mysql" --env "KAFKA_URL=kafka://127.0.0.1:9092/order" kafka.wasm 2>&1 &
```

The server log will appear in the `nohup.out` file.

## See it in action

The ETL program (i.e., the server from above) has already created and connected to the `order` queue in Redpanda when it started. You can produce data from the `order` topic using the [rpk](https://docs.redpanda.com/docs/platform/quickstart/rpk-install/) CLI tool. The ETL program will receive the data from the queue, parse it, process it, and save it to the database.

```bash
cat order.json | rpk topic produce order
```

> You can also use the `rpk` command to create a new topic for the ETL program to listen to. Just do `rpk topic create order`.

You can now log into the database to see the `orders` table and its content.

## Docker

With WasmEdge support in Docker, you can also use Docker Compose to build and start this multi-container application in a single command without installing any dependencies.

* [Install Docker Desktop + Wasm (Beta)](https://docs.docker.com/desktop/wasm/)
* or [Install Docker CLI + Wasm](https://github.com/chris-crone/wasm-day-na-22/tree/main/server)

Then, you just need to type one command.

```bash
docker compose up
```

This will build the Rust source code, run the Wasm server for the ETL function, and startup contaniners for a Redpanda server and a MySQL backing database. 

To try it out, log into the Redpanda container and send a message to the queue topic `order` as follows.

```bash
$ docker compose exec redpanda /bin/bash
redpanda@1add2615774b:/$ cd /app
redpanda@1add2615774b:/app$ cat order.json | rpk topic produce order
Produced to partition 0 at offset 0 with timestamp 1667922788523.
```

To see the data in the database container, you can use the following commands.

```bash
$ docker compose exec db /bin/bash
root@c97c472db02e:/# mysql -u root -pwhalehello mysql
mysql> select * from orders;
... ...
```
