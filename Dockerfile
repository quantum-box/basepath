# syntax=docker/dockerfile:1
ARG BUILDPLATFORM
ARG TARGETPLATFORM

FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS web-builder
WORKDIR /source
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY . .
RUN npm run build

FROM --platform=$BUILDPLATFORM rust:1.95-bookworm AS api-builder
WORKDIR /source
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
      gcc-x86-64-linux-gnu \
      libc6-dev-amd64-cross \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-unknown-linux-gnu
COPY api ./api
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
ENV CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc
ENV AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar
RUN cargo build --locked --release \
      --target x86_64-unknown-linux-gnu \
      --manifest-path api/Cargo.toml \
    && mkdir -p /source/runtime-data

FROM --platform=$TARGETPLATFORM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=api-builder /source/api/target/x86_64-unknown-linux-gnu/release/pathbase-api /app/pathbase-api
COPY --from=web-builder /source/dist/client /app/web
COPY --chown=10001:10001 --from=api-builder /source/runtime-data/ /app/data/
ENV PATHBASE_WEB_ROOT=/app/web
ENV PATHBASE_DB=/app/data/pathbase.sqlite3
ENV HOME=/tmp
EXPOSE 8080
USER 10001:10001
ENTRYPOINT ["/app/pathbase-api"]
