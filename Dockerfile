FROM rust:1.97-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations
COPY legal ./legal
RUN cargo build --release --locked

FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/andrescastiglia/agora" \
      org.opencontainers.image.description="Backend de Agora para grupos oficiales de WhatsApp" \
      org.opencontainers.image.licenses="MIT"
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        antiword \
        ca-certificates \
        curl \
        libreoffice-calc \
        libreoffice-writer \
        poppler-utils \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 agora
COPY --from=builder /app/target/release/agora /usr/local/bin/agora
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
USER agora
CMD ["agora"]
