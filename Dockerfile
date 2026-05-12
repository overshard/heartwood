# syntax=docker/dockerfile:1
# ----- builder -----
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

COPY --from=oven/bun:alpine /usr/local/bin/bun /usr/local/bin/bun

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
FROM alpine:3.23

# git is required at runtime: clone endpoints shell out to `git http-backend`
# (the canonical CGI), and the commit-diff view shells out to `git show`.
# ca-certificates so any outbound TLS just works.
RUN apk add --no-cache git ca-certificates

WORKDIR /app

COPY --from=builder /app/heartwood ./heartwood
COPY --from=builder /app/dist ./dist
COPY templates ./templates

RUN addgroup -S -g 1000 app && \
    adduser -S -h /app -s /sbin/nologin -u 1000 -G app app && \
    chown -R app:app /app
USER app

ENV PORT=8000
ENV HEARTWOOD_REPO_ROOT=/srv/git
EXPOSE 8000

CMD ["./heartwood"]
