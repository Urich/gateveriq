FROM ubuntu:24.04 AS build
RUN apt-get update && apt-get install -y --no-install-recommends build-essential curl ca-certificates && rm -rf /var/lib/apt/lists/*
ARG RUST_VERSION=1.93.0
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION}
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
COPY assets ./assets
RUN cargo build --release --locked

FROM ubuntu:24.04
LABEL org.opencontainers.image.source="https://github.com/Urich/gateveriq" \
      org.opencontainers.image.description="Self-hosted proof-of-work verification service"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 --create-home gateveriq && install -d -o gateveriq -g gateveriq -m 700 /data
COPY --from=build /build/target/release/gateveriq /usr/local/bin/gateveriq
COPY --chmod=755 docker/entrypoint.sh /usr/local/bin/docker-entrypoint.sh
USER 10001:10001
WORKDIR /data
VOLUME ["/data"]
EXPOSE 9080
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s CMD curl --fail --silent http://127.0.0.1:9080/health || exit 1
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["serve"]
