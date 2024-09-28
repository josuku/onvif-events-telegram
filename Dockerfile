FROM rust:1.83-slim-bookworm

RUN apt update && apt install -y libssl-dev pkg-config

WORKDIR /usr/src/my-app

COPY src src
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY config.yaml config.yaml

RUN cargo build

CMD cargo run config.yaml
