# ---------- Build ----------
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

# Dependencias de compilación:
#  - libpq-dev: Diesel (feature "postgres") enlaza contra libpq
#  - build-essential + pkg-config: compilar libwebp-sys (crate `webp`)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq-dev build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY . .
# Las migraciones se embeben en el binario en tiempo de compilación.
RUN cargo build --release --locked

# ---------- Runtime ----------
FROM debian:bookworm-slim
WORKDIR /app

# libpq5: requerido por Diesel en ejecución. ca-certificates: validar TLS
# (Mercado Pago, Cloudinary, Blue Express, SMTP) con rustls.
RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq5 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/jarstoreapi /usr/local/bin/jarstoreapi

# En contenedores hay que escuchar en 0.0.0.0; Render inyecta PORT.
ENV HOST=0.0.0.0
EXPOSE 8080

CMD ["jarstoreapi"]
