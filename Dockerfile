FROM lukemathwalker/cargo-chef:latest-rust-1-alpine3.22 AS chef
WORKDIR /app
RUN apk add --no-cache lld clang openssl-dev

FROM chef AS planner
COPY . .
# Compute a lock-like file for our project
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build our project dependencies, not our application!
RUN cargo chef cook --release --recipe-path recipe.json
# Up to this point, if our dependency tree stays the same,
# all layers should be cached.
COPY . .
ENV SQLX_OFFLINE=true
# Build our project
RUN cargo build --release --bin farms

FROM alpine:3.22 AS runtime
WORKDIR /app
RUN apk add --no-cache openssl ca-certificates
COPY --from=builder /app/target/release/farms farms
COPY configuration configuration

ENV APP_ENVIRONMENT=production
EXPOSE 8000

ENTRYPOINT ["./farms"]
