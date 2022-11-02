#!/usr/bin/env bash
BIN=target/armv7-unknown-linux-musleabi/release/tcp_transparent_repeater
docker run --rm -it \
    -v /root/.cargo/registry:/root/.cargo/registry \
    -v "$(pwd)":/home/rust/src \
    messense/rust-musl-cross:armv7-musleabi \
    cargo build --release
cp $BIN ttr
upx --best ttr
echo "The compilation result is located in $(realpath ttr)"

