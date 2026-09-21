# Build from the token-service directory:
#   cd token-service && docker build -t token-service .

FROM rust:1-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release \
    && strip target/release/token-service

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 65532 --no-create-home --shell /usr/sbin/nologin app
COPY --from=builder /src/target/release/token-service /usr/local/bin/token-service
ENV PORT=8080
EXPOSE 8080
USER 65532:65532
CMD ["token-service"]
