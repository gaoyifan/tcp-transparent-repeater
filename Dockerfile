FROM rust:alpine AS builder
RUN apk update
RUN apk add --no-cache musl-dev

WORKDIR /home/rust/ttr/
ADD Cargo.toml Cargo.lock ./
ADD src/ ./src/
RUN cargo build --release --target=$(uname -m)-unknown-linux-musl

FROM alpine
COPY --from=builder /home/rust/ttr/target/*-unknown-linux-musl/release/tcp_transparent_repeater /usr/local/bin/ttr
ENTRYPOINT ["/usr/local/bin/ttr"]
