FROM rust:1.83-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin logos

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/logos /usr/local/bin/
COPY data /app/seed
ENV LOGOS_DATA=/data
EXPOSE 3000
COPY docker-entry.sh /docker-entry.sh
RUN chmod +x /docker-entry.sh
ENTRYPOINT ["/docker-entry.sh"]
