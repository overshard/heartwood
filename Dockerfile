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
    cp target/release/repos /app/repos

# ----- runtime -----
FROM alpine:3.23

# git-daemon is the apk that ships /usr/libexec/git-core/git-http-backend
# (the plain `git` apk omits it; the package name is misleading, we don't
# actually run the daemon). http-backend powers the smart-HTTP clone
# endpoint; `git` itself is needed for the `git show` subprocess in
# diff_commit. ca-certificates so any outbound TLS just works.
RUN apk add --no-cache git git-daemon ca-certificates

# Bare repos under /srv/git are root-owned on the host; the web process
# runs as UID 1000. Without this, git's CVE-2022-24765 ownership check
# refuses every operation with "dubious ownership". The read-only bind
# mount in docker-compose.yml is the real safety control here.
RUN git config --system --add safe.directory '*'

WORKDIR /app

COPY --from=builder /app/repos ./repos
COPY --from=builder /app/dist ./dist
COPY templates ./templates

RUN addgroup -S -g 1000 app && \
    adduser -S -h /app -s /sbin/nologin -u 1000 -G app app && \
    chown -R app:app /app
USER app

ENV PORT=8000
ENV REPOS_REPO_ROOT=/srv/git
EXPOSE 8000

CMD ["./repos"]
