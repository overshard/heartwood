# Repos

A minimal, self-hosted web frontend for browsing the bare git repos on a
single-operator server. I built it to replace GitHub as the place my code is
publicly visible: a small, public, read-only window into a directory of bare
repos, with nothing else attached.

It is a single-binary axum service. No database, no auth, no pull requests, no
issues. Repo metadata is read live from the bare repos via `gix` (gitoxide).
The commit-diff view shells out to `git show --patch`, and `git clone` over
HTTPS works through `git http-backend` as a CGI subprocess. Everything else is
in-process.


## Features

- Repo list auto-discovered from a directory of `*.git` bare repos
- Per-repo landing page: README, recent commits, clone URL, default branch
- Tree / blob browser with syntect syntax highlighting
- Commit log and per-commit unified-diff view (rename detection on)
- Atom feed of recent commits per repo
- Read-only `git clone` over HTTPS (`/info/refs` + `/git-upload-pack`)
- Pushes are intentionally 405; the only way code arrives is `git push server master`


## Stack

| Concern        | Crate / Tool                                          |
|----------------|-------------------------------------------------------|
| Web framework  | axum + tokio                                          |
| Git read       | gix (gitoxide) for browse, log, tree, blob, atom      |
| Git serve      | `git http-backend` subprocess for clone               |
| Diff           | `git show --patch` subprocess + in-house unified parser |
| Templates      | minijinja                                             |
| Markdown       | pulldown-cmark (tables, footnotes, strikethrough, tasklists) |
| Highlighting   | syntect (`default-fancy`, base16-eighties.dark theme) |
| Static assets  | Vite + Bun, SCSS, JetBrains Mono (self-hosted via `@fontsource`) |


## Requirements

Production runs through Docker (see `Dockerfile`). The only runtime
dependencies are `git` and `ca-certificates` on top of `alpine:3.23`.

For local development:

- rust (cargo) for the backend
- bun for the frontend bundler (Vite)
- `git` on `PATH` (repos shells out to it for `http-backend` and `show`)


## Running locally

    cp samplefiles/env.sample .env  # optional; defaults are fine for dev
    make seed                        # one-time: synthesize 8 fake bare repos in fixtures/git/
    make run                         # vite watch + cargo run on port 8000

`make run` does not seed automatically; a fresh checkout shows an empty repo
list until you run `make seed` once. The seed step is opt-in so you can also
point `REPOS_REPO_ROOT` at a real directory and skip it entirely.

`make seed` runs the `seed` bin (see `src/bin/seed.rs`), which synthesizes
fake-but-realistic bare git repos under `fixtures/git/`. It picks a mix of
archetypes (Rust crate, TS lib, Python package, markdown blog, dotfiles) with
realistic file shapes, commit messages drawn from per-archetype corpora, and
~30 days of history across five rotating authors. Deterministic: the same
`--seed` value always produces the same set of repos.

Override the defaults inline:

    make seed COUNT=12 DAYS=45         # more repos, longer history
    make seed SEED=42                  # different deterministic mix

`make seed` is idempotent: existing repo directories are left alone, so
re-running is cheap. `make seed-reset` wipes `fixtures/git/` first.


## Configuration

All config comes from environment variables (loaded from `.env` via `dotenvy`):

| Variable | Required | Purpose |
|---|---|---|
| `PORT` | no (default `8000`) | HTTP listen port |
| `REPOS_ROOT` | no (default `.`) | Project root (where `templates/` and `dist/` live) |
| `REPOS_REPO_ROOT` | no (default `/srv/git`) | Directory of `<name>.git/` bare repos |
| `REPOS_CLONE_BASE` | no (default `https://repos.bythewood.me`) | Public origin used in clone URLs and atom self-links |
| `REPOS_TITLE` | no (default `repos`) | Topbar title |
| `REPOS_TAGLINE` | no (default empty) | Optional topbar tagline; hidden when empty |
| `BASE_URL` | no | `<base href>` if served on a subpath |

In dev, `make run` sets `REPOS_REPO_ROOT` to `./fixtures/git` so you don't
need a real `/srv/git/`.


## Make targets

| Target | What it does |
|---|---|
| `make run` (default) | Vite watch + `cargo run` on port 8000 |
| `make build` | Vite assets + release binary (`target/release/repos`) |
| `make start` | Run the release binary (after `make build`) |
| `make seed` | Synthesize fake bare repos under `fixtures/git/` (idempotent, opt-in) |
| `make seed-reset` | Wipe and re-synthesize `fixtures/git/` |
| `make push` | `git push` to every configured remote |
| `make clean` | Remove `target/`, `dist/`, and `frontend/node_modules/` (leaves fixtures) |

`seed` / `seed-reset` accept `COUNT=`, `DAYS=`, and `SEED=` overrides (see
Running locally).

There are no tests or linters configured.


## Key Routes

- `/`: repo list (auto-discovered from `REPOS_REPO_ROOT`, sorted by most-recent HEAD)
- `/<name>`: repo landing (README, recent commits, clone URL)
- `/<name>/log`: commit log (default branch unless `?rev=` given, `?limit=` up to 500)
- `/<name>/commit/<sha>`: single commit + unified diff
- `/<name>/tree/<rev>[/<path>]`: file browser
- `/<name>/blob/<rev>/<path>`: file view with syntax highlighting
- `/<name>/raw/<rev>/<path>`: raw blob bytes
- `/<name>/atom.xml`: atom feed of recent commits
- `/<name>.git/info/refs` and `/<name>.git/git-upload-pack`: smart HTTP clone
- `/static/*`: Vite assets (1y cache header)


## Production deploy

Same `git push server master` flow used by the rest of my projects:

Server:

    apk update && apk upgrade && apk add docker docker-compose caddy git iptables ip6tables ufw
    ufw allow 22/tcp && ufw allow 80/tcp && ufw allow 443/tcp && ufw --force enable
    rc-update add docker boot && service docker start
    mkdir -p /srv/git/repos.git && cd /srv/git/repos.git && git init --bare

Local:

    git remote add server root@repos.example.com:/srv/git/repos.git
    git push --set-upstream server master

Server:

    mkdir -p /srv/docker && cd /srv/docker && git clone /srv/git/repos.git repos && cd /srv/docker/repos
    cp samplefiles/Caddyfile.sample /etc/caddy/Caddyfile
    cp samplefiles/env.sample .env  # edit REPOS_CLONE_BASE and REPOS_TITLE if you want
    cp samplefiles/post-receive.sample /srv/git/repos.git/hooks/post-receive && chmod +x /srv/git/repos.git/hooks/post-receive
    docker-compose up --build --detach
    rc-update add caddy boot && service caddy start

The host's `/srv/git/` is bind-mounted into the container read-only so the
web process can't damage a bare repo, even accidentally. Pushes still arrive
the usual way: SSH to the bare repo, post-receive hook runs the deploy.


## Backups

All your code already lives in `/srv/git/*.git/`; back that up and you have a
complete backup. There is no application database to preserve.


## Support

I won't be providing user support for this project. I'm happy to accept good
pull requests and fix bugs but I don't have time to help people run or use
this project.
