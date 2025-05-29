ARG ARCH=x86_64
FROM messense/rust-musl-cross:${ARCH}-musl AS builder
ADD Cargo.toml Cargo.lock /home/rust/src/
ADD src/ /home/rust/src/src/
RUN cargo check
RUN cargo build --release

FROM alpine
ARG ARCH=x86_64
COPY --from=builder /home/rust/src/target/${ARCH}-unknown-linux-musl/release/tcp_transparent_repeater /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/tcp_transparent_repeater"]
