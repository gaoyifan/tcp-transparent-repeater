FROM rust:alpine as builder
RUN apk update
RUN apk add --no-cache musl-dev

WORKDIR /home/rust/ttr/
ADD ./ ./
RUN cargo build --release

FROM alpine
COPY --from=builder /home/rust/ttr/target/x86_64-unknown-linux-musl/release/tcp_transparent_repeater /usr/local/bin/ttr
ENTRYPOINT ["/usr/local/bin/ttr"]
