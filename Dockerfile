FROM debian:9 as builder

ARG TOOLCHAIN=nightly

RUN apt-get update && \
    apt-get install -y \
        build-essential \
        cmake \
        curl \
        file \
        git \
        musl-tools \
        xutils-dev \
        && \
    apt-get clean && rm -rf /var/lib/apt/lists/*
ARG HOME=/root
RUN mkdir -p $HOME/libs $HOME/src
ENV PATH=$HOME/.cargo/bin:/usr/local/musl/bin:/usr/local/bin:/usr/bin:/bin
RUN curl https://sh.rustup.rs -sSf | \
    sh -s -- -y --default-toolchain $TOOLCHAIN && \
    rustup target add x86_64-unknown-linux-musl
ADD .docker/cargo-config.toml $HOME/.cargo/config
WORKDIR $HOME/src

ADD Cargo.toml Cargo.lock $HOME/src/
ADD src $HOME/src/src/
RUN cargo check
RUN cargo build --release
RUN strip target/x86_64-unknown-linux-musl/release/tcp_transparent_repeater



FROM scratch
COPY --from=builder /root/src/target/x86_64-unknown-linux-musl/release/tcp_transparent_repeater /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/tcp_transparent_repeater"]
