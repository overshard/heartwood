# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A minimal web frontend for the bare git repos at `/srv/git/`. Single-operator,
public read-only. Each repo gets a landing page (README + recent commits +
clone URL), a tree/blob browser with syntax highlighting, a commit log, a
unified-diff view per commit, and an Atom feed. Clone over HTTPS works via
`git http-backend`. No pull requests, no issues, no auth, no database.

Built to replace Github as the place this user's code is publicly visible.

## Commands

- **Dev server:** `make run` (Vite watch + cargo run concurrently on port 8000)
- **Production build:** `make build` (Vite assets + release binary)
- **Run release binary:** `make start`
- **Seed local fixtures:** `make seed` (runs `cargo run --bin seed`, which
  synthesizes fake-but-realistic bare repos under `fixtures/git/` from five
  archetypes: Rust crate, TS lib, Python package, markdown blog, dotfiles,
  with ~30 days of history and five rotating authors). Opt-in: `make run`
  does not depend on it, so a fresh checkout shows an empty repo list until
  you run `make seed` once. Idempotent: existing repo dirs are left alone.
  `make seed-reset` wipes and regenerates. Override with
  `COUNT=12 DAYS=45 SEED=42`.
- **Docker build:** `sudo docker build .`

There are no tests or linters configured.

## Architecture

**Backend:** Single-binary axum app (`src/main.rs`). No DB, no auth. Repo
metadata is read live from the bare repos via `gix` (gitoxide); the
commit-diff view shells out to `git show --patch` and parses the unified-
diff output. Templates are minijinja, rendered server-side. State is just
the template environment and a `Config` struct.

**git layer (`src/git.rs`):** Wrappers over `gix` that pre-shape data for
templates. Functions: `discover` (walk repo root, return `RepoSummary`s
sorted by most-recent HEAD), `open` / `resolve_path` (with traversal
checks), `repo_summary`, `commit_info`, `recent_commits`, `list_tree`,
`read_blob`, `read_readme`, `resolve_rev`, and `diff_commit` (the one
function that shells out to git; everything else is gix in-process). The
`diff_commit` invocation passes `-c core.quotePath=false` so non-ASCII
paths in the `diff --git a/... b/...` header parse correctly.

**Markdown (`src/markdown.rs`):** pulldown-cmark with tables, footnotes,
strikethrough, task-lists, and smart-punctuation. Used to render READMEs
on the repo landing.

**Syntax highlighting (`src/highlight.rs`):** syntect with the default-fancy
feature set, themed with `base16-eighties.dark`. SyntaxSet and Theme are
loaded once via `OnceLock` so each file view doesn't re-parse them.

**Smart-HTTP clone (`src/http_backend.rs` + `src/routes/clone.rs`):** The
`/info/refs` and `/git-upload-pack` routes spawn `git http-backend` as a
CGI subprocess. CGI env vars are set from the axum request (PATH_INFO,
QUERY_STRING, REQUEST_METHOD, CONTENT_TYPE, GIT_PROJECT_ROOT,
GIT_HTTP_EXPORT_ALL, plus a few HTTP_* passthrough headers). Request body
is piped into stdin; CGI headers are parsed off stdout, the rest is
streamed back as the response body. `/git-receive-pack` is wired but
returns 405 (heartwood is read-only).

**Templates (`templates/`):** Jinja2-compatible. `base.html` is the shell;
`index.html` is the repo list; `repo.html` is README + recent commits +
clone URL + nav; `tree.html` / `blob.html` are the file browser;
`log.html` / `commit.html` are commit views; `atom.xml` is the per-repo
feed; `not_found.html` is the themed 404 shell.

**Jinja2-faithful HTML formatter:** Like every other Rust project in this
workspace, `src/templates.rs` installs a custom minijinja formatter so `/`
is not escaped to `&#x2f;` (Vite asset URLs come through clean).

**Frontend pipeline:** Vite from `frontend/`. Single entry
(`frontend/static_src/index.js`) imports SCSS + JetBrains Mono via
@fontsource. Output filenames are content-hashed and served at `/static/`;
templates resolve them via `vite_asset` reading `dist/.vite/manifest.json`.
The favicon lives in `frontend/static_src/public/favicon.svg`, which Vite
copies into `dist/` at build time; the runtime `/favicon.ico` and
`/favicon.svg` handlers in `src/routes/seo.rs` read from `dist/favicon.svg`
(not the `frontend/` source tree, which isn't shipped in the image).

**Request logging:** `src/middleware.rs::log_requests` prints
`time METHOD STATUS latency path` per request with ANSI-colored status.
Sub-microsecond cost.

**404s:** `src/render.rs::not_found` is the shared helper every route uses
on a missing repo / bad revspec / bad path. The router fallback (in
`src/middleware.rs::not_found`) renders the same template, so any unmatched
URL gets the themed shell instead of a plain-text 404.

## Configuration

All via env vars (defaults shown):

- `PORT=8000` — HTTP listen port.
- `HEARTWOOD_ROOT=.` — project root (where `templates/` and `dist/` live).
- `HEARTWOOD_REPO_ROOT=/srv/git` — directory containing `<name>.git/` bare repos.
- `HEARTWOOD_CLONE_BASE=https://heartwood.bythewood.me` — public origin used
  in clone URLs and atom self-links.
- `HEARTWOOD_TITLE=heartwood` and `HEARTWOOD_TAGLINE=every commit a ring`
  for the topbar.
- `BASE_URL=` — optional `<base href>` if served on a subpath.

In dev, `make run` sets `HEARTWOOD_REPO_ROOT=./fixtures/git` so you don't
need a real `/srv/git/`.

## Layout

```
heartwood/
├── Cargo.toml, Cargo.lock
├── Makefile, README.md, LICENSE.md, CLAUDE.md
├── src/
│   ├── main.rs            # entry: env, server boot
│   ├── app.rs             # AppState + router
│   ├── render.rs          # render() + shared not_found() helper
│   ├── middleware.rs      # request log + router fallback (renders not_found.html)
│   ├── templates.rs       # minijinja env, vite_asset, jinja2 formatter, filters
│   ├── git.rs             # gix wrappers (discover, open, log, tree, blob, diff)
│   ├── markdown.rs        # pulldown-cmark wrapper for READMEs
│   ├── highlight.rs       # syntect wrapper for blob views
│   ├── http_backend.rs    # CGI subprocess for git http-backend
│   ├── bin/
│   │   └── seed.rs        # `cargo run --bin seed` to populate fixtures/git/
│   └── routes/
│       ├── index.rs       # /
│       ├── repo.rs        # /:name (overview)
│       ├── log.rs         # /:name/log
│       ├── commit.rs      # /:name/commit/:sha
│       ├── tree.rs        # /:name/tree/:rev[/*path]
│       ├── blob.rs        # /:name/blob/:rev/*path and /:name/raw/:rev/*path
│       ├── atom.rs        # /:name/atom.xml
│       ├── clone.rs       # /:name.git/info/refs, /:name.git/git-upload-pack
│       └── seo.rs         # /favicon.ico, /favicon.svg, /robots.txt
├── templates/             # minijinja templates (incl. not_found.html)
├── frontend/              # JS pipeline (package.json, vite.config.js, static_src/)
├── dist/                  # vite output (gitignored, served at /static/)
├── fixtures/git/          # local seed bare repos (gitignored, `make fixtures`)
├── target/                # cargo output (gitignored)
├── Dockerfile, docker-compose.yml
└── samplefiles/           # Caddyfile, env, post-receive samples
```

## Key Routes

- `/` — repo list (auto-discovered from `HEARTWOOD_REPO_ROOT`)
- `/:name` — repo landing: README + recent commits + clone URL
- `/:name/log` — commit log (default branch unless `?rev=` given, `?limit=` up to 500)
- `/:name/commit/:sha` — single commit + unified diff
- `/:name/tree/:rev` and `/:name/tree/:rev/*path` — file browser
- `/:name/blob/:rev/*path` — file view with syntax highlighting
- `/:name/raw/:rev/*path` — raw file content
- `/:name/atom.xml` — atom feed of recent commits
- `/:name.git/info/refs` and `/:name.git/git-upload-pack` — smart HTTP clone
- `/static/*` — Vite assets (1y cache header)
- `/favicon.ico`, `/favicon.svg`, `/robots.txt` — seo

## Tooling

- **Rust deps:** managed with `cargo` (`Cargo.toml`, `Cargo.lock`)
- **JS deps:** managed with `bun`, run from `frontend/` (`frontend/package.json`, `frontend/bun.lock`)
- **Production:** Docker (`rust:alpine` builder + `alpine:3.23` runtime).
  Runtime image installs `git` (needed for `git http-backend` and the
  `git show` subprocess used by the commit-diff view) and
  `ca-certificates`. Deployed via `git push server master` triggering the
  standard post-receive hook. The host's `/srv/git/` is mounted read-only
  into the container so the web process can't corrupt a bare repo even
  accidentally.

## Why some choices

- **gix for browse, shell out for clone + diff:** gix is great for reading
  repos in-process but its server side isn't production-ready, so smart-
  HTTP serving uses `git http-backend` (git's own CGI program, the
  reference implementation). The commit-diff view shells out to `git show`
  for the same reason: git's unified-diff renderer is the canonical
  implementation and is already on the runtime image for `http-backend`.
- **No DB:** All repo metadata is read live from the bare repos. With a
  handful of personal repos this is fast enough; if the count grows we can
  add a startup-time index later.
- **Read-only host mount:** Defense in depth. The web process never needs
  to write to a bare repo; `:ro` makes accidental damage impossible.

## Development tools

- Playwright MCP is available for browser testing. Use it to verify
  visual changes after touching SCSS or templates. Start the dev server
  first with `make run`. Save screenshots to `/tmp` and delete them after
  reviewing.
- The dev environment runs inside a Docker container with port 8000
  mapped to the host.
