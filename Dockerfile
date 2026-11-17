# syntax=docker/dockerfile:1
ARG RUST_VERSION=1.93.0
ARG ALPINE_VERSION=3.23
FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS build
RUN apk add --no-cache build-base
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
COPY assets ./assets
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release --locked && \
    install -D target/release/gateveriq /out/gateveriq && \
    strip /out/gateveriq

FROM alpine:${ALPINE_VERSION}
LABEL org.opencontainers.image.source="https://github.com/Urich/gateveriq" \
      org.opencontainers.image.description="Self-hosted proof-of-work verification service"
RUN apk add --no-cache ca-certificates curl libgcc && \
    addgroup -S -g 10001 gateveriq && \
    adduser -S -D -u 10001 -G gateveriq gateveriq && \
    mkdir /data && chown gateveriq:gateveriq /data && chmod 700 /data
COPY --from=build /out/gateveriq /usr/local/bin/gateveriq
COPY --chmod=755 docker/entrypoint.sh /usr/local/bin/docker-entrypoint.sh
ENV GATEVERIQ_INTERNAL_LISTEN=0.0.0.0:9081
USER 10001:10001
WORKDIR /data
VOLUME ["/data"]
EXPOSE 9080 9081
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s CMD curl --fail --silent http://127.0.0.1:9080/health || exit 1
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["serve"]
