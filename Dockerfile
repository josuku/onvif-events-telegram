FROM rust:1.89-slim-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked
WORKDIR /usr/src/app
 
FROM chef AS planner
COPY . . 
RUN cargo chef prepare --recipe-path recipe.json
 
FROM chef AS builder
COPY --from=planner /usr/src/app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
 
COPY . .
RUN cargo build --release --bin onvif-events-telegram
 
FROM debian:bookworm-slim AS runtime
 
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*
 
WORKDIR /app
 
COPY --from=builder /usr/src/app/target/release/onvif-events-telegram ./onvif-events-telegram
 
COPY --from=builder /usr/src/app/ultralytics-runtime/linux/libonnxruntime.so ./libonnxruntime.so
ENV ORT_DYLIB_PATH=/app/libonnxruntime.so
ENV LD_LIBRARY_PATH=/app
ENV TZ=Europe/Madrid
 
COPY --from=builder /usr/src/app/models ./models
 
COPY --from=builder /usr/src/app/config.yaml.example ./config.yaml.example
 
RUN mkdir -p logs
 
EXPOSE 3702/udp
 
CMD ["./onvif-events-telegram", "config.yaml"]
 