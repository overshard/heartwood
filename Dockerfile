# syntax=docker/dockerfile:1
# ----- builder -----
FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY --from=oven/bun:debian /usr/local/bin/bun /usr/local/bin/bun

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY frontend ./frontend

RUN cd frontend && bun install --frozen-lockfile && bun run build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/heartwood /app/heartwood

# ----- runtime -----
# debian-slim, not alpine: alpine's `git` apk omits `git-http-backend`,
# which the smart-HTTP clone endpoint depends on. The other Rust services
# in this workspace stay on alpine; heartwood is the only one that needs
# the CGI binary.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/heartwood ./heartwood
COPY --from=builder /app/dist ./dist
COPY templates ./templates

RUN groupadd -g 1000 app && \
    useradd -u 1000 -g app -d /app -s /usr/sbin/nologin -M app && \
    chown -R app:app /app
USER app

ENV PORT=8000
ENV HEARTWOOD_REPO_ROOT=/srv/git
EXPOSE 8000

CMD ["./heartwood"]
