# Build context is the repo root (workspace Cargo.lock needed for resolution):
#   docker build -f server/Dockerfile .

FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev build-base
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY client/Cargo.toml client/Cargo.toml
COPY server/Cargo.toml server/Cargo.toml
COPY client/src client/src
COPY server/src server/src
COPY server/migrations server/migrations
COPY server/assets server/assets

# sqlx::migrate! embeds migrations/*.sql into the binary at compile time —
# no need to ship the migrations/ directory in the runtime image.
RUN cargo build --release -p work-dash-server

FROM alpine:3.20
RUN apk add --no-cache ca-certificates tzdata
WORKDIR /app
COPY --from=builder /app/target/release/work-dash-server ./work-dash-server
COPY server/assets ./assets

ENV DATABASE_URL=sqlite:///data/work-dash.db
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["./work-dash-server"]
