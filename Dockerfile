# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
COPY static ./static
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd -r -s /bin/false kirin
WORKDIR /srv
RUN mkdir -p /srv/data && chown kirin:kirin /srv/data
COPY --from=builder /app/target/release/kirin /usr/local/bin/kirin
COPY config.example.toml /etc/kirin/config.example.toml
USER kirin
EXPOSE 8080
ENV RUST_LOG=info
CMD ["/usr/local/bin/kirin", "/etc/kirin/config.toml"]
