FROM node:22-bookworm-slim AS web-builder
WORKDIR /source
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY . .
RUN npm run build

FROM rust:1.95-bookworm AS api-builder
WORKDIR /source
COPY api ./api
RUN cargo build --locked --release --manifest-path api/Cargo.toml

FROM debian:bookworm-slim AS runtime
RUN useradd --create-home --uid 10001 pathbase \
    && mkdir -p /app/data \
    && chown -R pathbase:pathbase /app
WORKDIR /app
COPY --from=api-builder /source/api/target/release/pathbase-api /app/pathbase-api
COPY --from=web-builder /source/dist/client /app/web
ENV PATHBASE_WEB_ROOT=/app/web
ENV PATHBASE_DB=/app/data/pathbase.sqlite3
EXPOSE 8080
USER pathbase
ENTRYPOINT ["/app/pathbase-api"]
