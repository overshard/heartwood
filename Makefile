CARGO ?= $(HOME)/.cargo/bin/cargo
PORT  ?= 8000

# In dev the bare repos at /srv/git live on the production server, not here.
# Use ./fixtures/git as the local repo root; `make seed` populates it so
# `make run` has something to show on the landing page.
REPOS_REPO_ROOT ?= $(CURDIR)/fixtures/git

.DEFAULT_GOAL := run
.PHONY: run build start clean push seed seed-reset

# Dev: Vite watch + cargo run concurrently. Both die on Ctrl+C.
# Seeding is opt-in (run `make seed` once); a fresh checkout shows an empty
# repo list until you do.
run: frontend/node_modules dist/.vite/manifest.json
	@trap 'kill 0' EXIT INT TERM; \
	(cd frontend && bun run dev) & \
	PORT=$(PORT) REPOS_REPO_ROOT=$(REPOS_REPO_ROOT) $(CARGO) run

# Production build (Vite assets + release binary)
build: frontend/node_modules
	cd frontend && bun run build
	$(CARGO) build --release

# Run the release binary (after `make build`)
start:
	PORT=$(PORT) ./target/release/repos

# Wipe regenerable output. Leaves fixtures/ alone so `make run` after `make
# clean` doesn't blow away your seeded repos.
clean:
	rm -rf target dist frontend/node_modules

push:
	git remote | xargs -I R git push R master

# Synthesize a handful of fake-but-realistic bare repos under fixtures/git/
# via the `seed` bin. Idempotent: existing repo dirs are left alone, so
# re-running is cheap. Override defaults inline, e.g. `make seed COUNT=12
# DAYS=45 SEED=42`. See src/bin/seed.rs for the archetypes and patches.
COUNT ?=
DAYS  ?=
SEED  ?=
SEED_ARGS = $(if $(COUNT),--count $(COUNT)) $(if $(DAYS),--days $(DAYS)) $(if $(SEED),--seed $(SEED))

seed:
	$(CARGO) run --quiet --bin seed -- $(SEED_ARGS)

# Wipe and regenerate from scratch. Use after editing seed.rs or when you
# want a different deterministic set (pair with SEED=<n>).
seed-reset:
	$(CARGO) run --quiet --bin seed -- --reset $(SEED_ARGS)

frontend/node_modules:
	cd frontend && bun install

dist/.vite/manifest.json: frontend/node_modules
	cd frontend && bun run build
