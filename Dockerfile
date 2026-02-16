# Stage 1: Build
FROM rust:1.84-bookworm AS builder
WORKDIR /usr/src/lazypaw
COPY . .
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/lazypaw/target/release/lazypaw /usr/local/bin/lazypaw
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/lazypaw"]
