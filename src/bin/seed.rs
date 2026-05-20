//! Generate fake-but-realistic bare git repos under `fixtures/git/` so the
//! repos landing page has something to render in dev. Each repo gets a
//! month of commit history with multiple authors, a per-archetype file shape
//! (Rust crate, TS lib, Python package, markdown blog, dotfiles), and commit
//! messages drawn from a per-archetype corpus.
//!
//! Deterministic: pass `--seed N` to reproduce the same set of repos. Default
//! seed is fine; the same invocation always yields the same fixtures.
//!
//! Usage:
//!   cargo run --bin seed
//!   cargo run --bin seed -- --count 12 --days 45
//!   cargo run --bin seed -- --reset      # wipe fixtures/git/ first

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_COUNT: usize = 8;
const DEFAULT_DAYS: i64 = 30;
const DEFAULT_SEED: u64 = 0xC0DE_FEED;
const DEST: &str = "fixtures/git";

// ---------- PRNG ------------------------------------------------------------

/// Xorshift64. Deterministic, zero deps, plenty good for picking from small
/// pools. Seeded from CLI so a given run is reproducible.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0xCAFE_BABE } else { seed })
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, max: usize) -> usize {
        (self.next_u64() as usize) % max.max(1)
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.range(items.len())]
    }
    /// Returns true with probability `n_in_10 / 10`.
    fn chance(&mut self, n_in_10: usize) -> bool {
        self.range(10) < n_in_10
    }
}

// ---------- authors ---------------------------------------------------------

struct Author {
    name: &'static str,
    email: &'static str,
}

const AUTHORS: &[Author] = &[
    Author { name: "Isaac Bythewood", email: "isaac@bythewood.me" },
    Author { name: "Anna Holm",       email: "anna@holm.dev" },
    Author { name: "Jules Sato",      email: "jules@sato.io" },
    Author { name: "Maren Akkerman",  email: "maren@akkerman.nl" },
    Author { name: "Felix Ortiz",     email: "felix@ortiz.codes" },
];

/// 70% Isaac, 30% one of the others. Mirrors the look of a personal repo with
/// occasional drive-by contributions.
fn pick_author(rng: &mut Rng) -> &'static Author {
    if rng.chance(7) {
        &AUTHORS[0]
    } else {
        &AUTHORS[1 + rng.range(AUTHORS.len() - 1)]
    }
}

// ---------- archetypes ------------------------------------------------------

/// A single mutation applied per commit, paired with the commit messages
/// that plausibly describe it. Bundling op + messages stops the message and
/// diff from drifting apart (a commit titled "switch to thiserror" that
/// actually appends "" to README is the giveaway that this is fake).
struct Patch {
    op: PatchOp,
    messages: &'static [&'static str],
}

enum PatchOp {
    /// Create the file with this body. If it already exists, the patch is a
    /// no-op and the seeder picks a different patch.
    Create { path: &'static str, body: &'static str },
    /// Append a single line. If the line is already the file's last line,
    /// the patch is a no-op and the seeder picks a different patch.
    Append { path: &'static str, line: &'static str },
}

struct Archetype {
    kind: &'static str,
    description: &'static str,
    names: &'static [&'static str],
    initial: &'static [(&'static str, &'static str)],
    patches: &'static [Patch],
}

// --- rust crate ---

const RUST_LIB_RS: &str = "//! {name}: a small Rust library.\n\
\n\
pub fn version() -> &'static str {\n\
    env!(\"CARGO_PKG_VERSION\")\n\
}\n\
\n\
#[cfg(test)]\n\
mod tests {\n\
    use super::*;\n\
\n\
    #[test]\n\
    fn version_is_set() {\n\
        assert!(!version().is_empty());\n\
    }\n\
}\n";

const RUST_CARGO_TOML: &str = "[package]\n\
name = \"{name}\"\n\
version = \"0.1.0\"\n\
edition = \"2021\"\n\
\n\
[dependencies]\n";

const RUST_README: &str = "# {name}\n\
\n\
A small Rust crate that does one thing well.\n\
\n\
The API surface is intentionally narrow: a handful of types and free\n\
functions, no async runtime baked in.\n\
\n\
## Quick example\n\
\n\
    use {snake}::version;\n\
\n\
    println!(\"{}\", version());\n\
\n\
## License\n\
\n\
BSD-2-Clause.\n";

const RUST_GITIGNORE: &str = "/target\nCargo.lock\n";

const RUST_PARSE_RS: &str = "//! Tiny hand-written parser. Greedy, not particularly fast, but the\n\
//! tokenizer is straightforward enough to step through in a debugger.\n\
\n\
pub fn parse(input: &str) -> Result<Vec<String>, ParseError> {\n\
    let mut out = Vec::new();\n\
    for token in input.split_whitespace() {\n\
        if token.is_empty() {\n\
            return Err(ParseError::Empty);\n\
        }\n\
        out.push(token.to_string());\n\
    }\n\
    Ok(out)\n\
}\n\
\n\
#[derive(Debug)]\n\
pub enum ParseError {\n\
    Empty,\n\
    Unterminated,\n\
}\n";

const RUST_ERROR_RS: &str = "use std::fmt;\n\
\n\
#[derive(Debug)]\n\
pub enum Error {\n\
    Io(std::io::Error),\n\
    Parse(crate::parse::ParseError),\n\
}\n\
\n\
impl fmt::Display for Error {\n\
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {\n\
        match self {\n\
            Error::Io(e) => write!(f, \"io: {}\", e),\n\
            Error::Parse(e) => write!(f, \"parse: {:?}\", e),\n\
        }\n\
    }\n\
}\n\
\n\
impl std::error::Error for Error {}\n";

const RUST_UTIL_RS: &str = "/// Pad `s` on the right with spaces up to `width`.\n\
pub fn pad_right(s: &str, width: usize) -> String {\n\
    if s.len() >= width { s.to_string() } else { format!(\"{:<w$}\", s, w = width) }\n\
}\n";

const RUST_BENCH_RS: &str = "#![feature(test)]\n\
extern crate test;\n\
\n\
use test::Bencher;\n\
use {name}::parse::parse;\n\
\n\
#[bench]\n\
fn parse_short(b: &mut Bencher) {\n\
    b.iter(|| parse(\"one two three four\"));\n\
}\n";

const RUST_ARCHETYPE: Archetype = Archetype {
    kind: "rust-crate",
    description: "a small rust library",
    names: &[
        "roman-runes",
        "axum-knife",
        "beam-walker",
        "sextant",
        "basalt",
        "copper-net",
        "runesmith",
        "oxide-quill",
        "ferroquill",
    ],
    initial: &[
        ("Cargo.toml", RUST_CARGO_TOML),
        ("src/lib.rs", RUST_LIB_RS),
        ("README.md", RUST_README),
        (".gitignore", RUST_GITIGNORE),
    ],
    patches: &[
        Patch {
            op: PatchOp::Create { path: "src/parse.rs", body: RUST_PARSE_RS },
            messages: &[
                "split parse into its own module",
                "first cut of the tokenizer",
                "carve parse out of lib.rs",
            ],
        },
        Patch {
            op: PatchOp::Create { path: "src/error.rs", body: RUST_ERROR_RS },
            messages: &[
                "tighten error type",
                "introduce Error enum",
                "give errors a Display impl",
            ],
        },
        Patch {
            op: PatchOp::Create { path: "src/util.rs", body: RUST_UTIL_RS },
            messages: &[
                "add pad_right util",
                "extract padding helper",
                "pull util out of lib.rs",
            ],
        },
        Patch {
            op: PatchOp::Create { path: "benches/parse.rs", body: RUST_BENCH_RS },
            messages: &["add bench skeleton", "bench parse on a short input"],
        },
        Patch {
            op: PatchOp::Append { path: "src/lib.rs", line: "pub mod parse;" },
            messages: &["wire parse into lib.rs", "expose parse module"],
        },
        Patch {
            op: PatchOp::Append { path: "src/lib.rs", line: "pub mod error;" },
            messages: &["expose error module", "wire error into lib.rs"],
        },
        Patch {
            op: PatchOp::Append { path: "src/lib.rs", line: "pub mod util;" },
            messages: &["expose util module", "promote util to pub"],
        },
        Patch {
            op: PatchOp::Append { path: "Cargo.toml", line: "thiserror = \"1\"" },
            messages: &["switch to thiserror for Error", "add thiserror dep"],
        },
        Patch {
            op: PatchOp::Append { path: "Cargo.toml", line: "anyhow = \"1\"" },
            messages: &["pull anyhow for the examples", "add anyhow dep"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Tested against rust 1.75 and current stable.",
            },
            messages: &["note MSRV in the readme", "mention tested rust versions"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "*.rs.bk" },
            messages: &["ignore rustfmt backup files"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "perf.data*" },
            messages: &["ignore perf data files"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "/criterion" },
            messages: &["ignore criterion output dir"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: ".envrc" },
            messages: &["ignore .envrc (direnv)"],
        },
        Patch {
            op: PatchOp::Append { path: "Cargo.toml", line: "serde = { version = \"1\", features = [\"derive\"] }" },
            messages: &["pull serde for the public types"],
        },
        Patch {
            op: PatchOp::Append { path: "Cargo.toml", line: "log = \"0.4\"" },
            messages: &["add a log facade"],
        },
        Patch {
            op: PatchOp::Append { path: "Cargo.toml", line: "regex = \"1\"" },
            messages: &["lean on regex for the trickier patterns"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "MSRV: rust 1.75. Older toolchains may compile but aren't tested.",
            },
            messages: &["pin MSRV in the readme"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Issues and patches welcome. The repo lives at git.bythewood.me/{name}.",
            },
            messages: &["link the source in the readme"],
        },
        Patch {
            op: PatchOp::Append { path: "src/lib.rs", line: "pub use parse::parse;" },
            messages: &["re-export parse() at the crate root"],
        },
        Patch {
            op: PatchOp::Create {
                path: "rustfmt.toml",
                body: "edition = \"2021\"\nmax_width = 100\nuse_field_init_shorthand = true\n",
            },
            messages: &["pin rustfmt settings"],
        },
        Patch {
            op: PatchOp::Create {
                path: "CHANGELOG.md",
                body: "# Changelog\n\n## Unreleased\n\n- Initial slice.\n",
            },
            messages: &["start a changelog"],
        },
        Patch {
            op: PatchOp::Create {
                path: ".cargo/config.toml",
                body: "[build]\nrustflags = [\"-D\", \"warnings\"]\n",
            },
            messages: &["fail the build on warnings"],
        },
        Patch {
            op: PatchOp::Create {
                path: "examples/parse_one.rs",
                body: "use {snake}::parse::parse;\n\nfn main() {\n    println!(\"{:?}\", parse(\"one two three\"));\n}\n",
            },
            messages: &["add a parse_one example"],
        },
        Patch {
            op: PatchOp::Append { path: "CHANGELOG.md", line: "- Tighten error type." },
            messages: &["changelog: error tightening"],
        },
        Patch {
            op: PatchOp::Append { path: "CHANGELOG.md", line: "- Promote util to pub." },
            messages: &["changelog: util pub"],
        },
        Patch {
            op: PatchOp::Append {
                path: "src/lib.rs",
                line: "/// Re-exports the canonical parser. See [`parse`] for the full API.",
            },
            messages: &["doc-comment the parse re-export"],
        },
    ],
};

// --- typescript lib ---

const TS_PKG_JSON: &str = "{\n\
  \"name\": \"{name}\",\n\
  \"version\": \"0.1.0\",\n\
  \"type\": \"module\",\n\
  \"main\": \"dist/index.js\",\n\
  \"types\": \"dist/index.d.ts\",\n\
  \"scripts\": {\n\
    \"build\": \"tsc\",\n\
    \"test\": \"bun test\"\n\
  }\n\
}\n";

const TS_TSCONFIG: &str = "{\n\
  \"compilerOptions\": {\n\
    \"target\": \"ES2022\",\n\
    \"module\": \"ESNext\",\n\
    \"moduleResolution\": \"bundler\",\n\
    \"declaration\": true,\n\
    \"outDir\": \"dist\",\n\
    \"strict\": true\n\
  },\n\
  \"include\": [\"src\"]\n\
}\n";

const TS_INDEX: &str = "export interface Options {\n\
  width?: number;\n\
  prefix?: string;\n\
}\n\
\n\
export function pad(s: string, opts: Options = {}): string {\n\
  const width = opts.width ?? 8;\n\
  const prefix = opts.prefix ?? \"\";\n\
  if (s.length >= width) return prefix + s;\n\
  return prefix + s + \" \".repeat(width - s.length);\n\
}\n";

const TS_README: &str = "# {name}\n\
\n\
Tiny TypeScript helper, zero runtime dependencies.\n\
\n\
## Install\n\
\n\
    bun add {name}\n\
\n\
## Use\n\
\n\
    import { pad } from \"{name}\";\n\
\n\
    pad(\"hi\", { width: 8 });\n";

const TS_GITIGNORE: &str = "node_modules\ndist\nbun.lock\n";

const TS_STRINGS_TS: &str = "export function capitalize(s: string): string {\n\
  if (!s) return s;\n\
  return s[0].toUpperCase() + s.slice(1);\n\
}\n\
\n\
export function kebab(s: string): string {\n\
  return s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`).replace(/^-/, \"\");\n\
}\n";

const TS_TEST: &str = "import { describe, it, expect } from \"bun:test\";\n\
import { pad } from \"./index\";\n\
\n\
describe(\"pad\", () => {\n\
  it(\"pads short strings to width\", () => {\n\
    expect(pad(\"hi\", { width: 5 })).toBe(\"hi   \");\n\
  });\n\
});\n";

const TS_ARCHETYPE: Archetype = Archetype {
    kind: "ts-lib",
    description: "a tiny typescript helper",
    names: &["spindrift", "kelp", "sailcloth", "dunelight", "vellum", "papyrus", "marblefall"],
    initial: &[
        ("package.json", TS_PKG_JSON),
        ("tsconfig.json", TS_TSCONFIG),
        ("src/index.ts", TS_INDEX),
        ("README.md", TS_README),
        (".gitignore", TS_GITIGNORE),
    ],
    patches: &[
        Patch {
            op: PatchOp::Create { path: "src/strings.ts", body: TS_STRINGS_TS },
            messages: &["add capitalize + kebab", "split string helpers into a module"],
        },
        Patch {
            op: PatchOp::Create { path: "src/index.test.ts", body: TS_TEST },
            messages: &["add bun:test smoke test", "tests: pad pads short strings"],
        },
        Patch {
            op: PatchOp::Append {
                path: "src/index.ts",
                line: "export * from \"./strings\";",
            },
            messages: &["re-export strings from the entrypoint", "wire strings into the public API"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Targets ES2022. Runs on bun, node 20+, and modern browsers.",
            },
            messages: &["note runtime targets in the readme"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "*.tsbuildinfo" },
            messages: &["ignore tsbuildinfo"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: ".turbo" },
            messages: &["ignore turbo cache"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "coverage" },
            messages: &["ignore coverage reports"],
        },
        Patch {
            op: PatchOp::Append {
                path: "src/index.ts",
                line: "export const VERSION = \"0.1.0\";",
            },
            messages: &["expose VERSION constant"],
        },
        Patch {
            op: PatchOp::Append {
                path: "package.json",
                line: "  \"keywords\": [\"strings\", \"utility\"],",
            },
            messages: &["add keywords to package.json"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Zero runtime dependencies. ESM-only.",
            },
            messages: &["note ESM-only in the readme"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Source lives at git.bythewood.me/{name}.",
            },
            messages: &["link the source"],
        },
        Patch {
            op: PatchOp::Create {
                path: "biome.json",
                body: "{\n  \"$schema\": \"https://biomejs.dev/schemas/1.9.0/schema.json\",\n  \"linter\": { \"enabled\": true },\n  \"formatter\": { \"indentStyle\": \"space\", \"indentWidth\": 2 }\n}\n",
            },
            messages: &["adopt biome for lint + format"],
        },
        Patch {
            op: PatchOp::Create {
                path: "CHANGELOG.md",
                body: "# Changelog\n\n## Unreleased\n\n- Initial slice.\n",
            },
            messages: &["start a changelog"],
        },
        Patch {
            op: PatchOp::Create {
                path: "src/numbers.ts",
                body: "export function clamp(n: number, lo: number, hi: number): number {\n  return Math.min(hi, Math.max(lo, n));\n}\n\nexport function lerp(a: number, b: number, t: number): number {\n  return a + (b - a) * t;\n}\n",
            },
            messages: &["add numeric helpers"],
        },
        Patch {
            op: PatchOp::Append {
                path: "src/index.ts",
                line: "export * from \"./numbers\";",
            },
            messages: &["re-export numeric helpers"],
        },
        Patch {
            op: PatchOp::Append { path: "CHANGELOG.md", line: "- Add strings module." },
            messages: &["changelog: strings"],
        },
        Patch {
            op: PatchOp::Append { path: "CHANGELOG.md", line: "- Add numbers module." },
            messages: &["changelog: numbers"],
        },
        Patch {
            op: PatchOp::Create {
                path: ".github/dependabot.yml",
                body: "version: 2\nupdates:\n  - package-ecosystem: npm\n    directory: \"/\"\n    schedule:\n      interval: weekly\n",
            },
            messages: &["wire dependabot for npm"],
        },
    ],
};

// --- python package ---

const PY_PYPROJECT: &str = "[project]\n\
name = \"{name}\"\n\
version = \"0.1.0\"\n\
requires-python = \">=3.11\"\n\
description = \"\"\n\
readme = \"README.md\"\n\
dependencies = []\n\
\n\
[build-system]\n\
requires = [\"hatchling\"]\n\
build-backend = \"hatchling.build\"\n";

const PY_INIT: &str = "from .core import run, Result\n\
\n\
__all__ = [\"run\", \"Result\"]\n\
__version__ = \"0.1.0\"\n";

const PY_CORE: &str = "from dataclasses import dataclass\n\
\n\
\n\
@dataclass(frozen=True, slots=True)\n\
class Result:\n\
    ok: bool\n\
    value: str | None = None\n\
\n\
\n\
def run(query: str) -> Result:\n\
    if not query.strip():\n\
        return Result(ok=False)\n\
    return Result(ok=True, value=query.strip().lower())\n";

const PY_README: &str = "# {name}\n\
\n\
A small Python package. Pure stdlib at the core; optional extras for the CLI.\n\
\n\
## Install\n\
\n\
    uv pip install {name}\n\
\n\
## Use\n\
\n\
    from {snake} import run\n\
\n\
    print(run(\"  hello  \").value)  # 'hello'\n";

const PY_GITIGNORE: &str = "__pycache__\n.venv\ndist\n*.egg-info\n";

const PY_TEST: &str = "from {snake} import run\n\
\n\
\n\
def test_run_strips_and_lowercases():\n\
    assert run(\"  Hello  \").value == \"hello\"\n\
\n\
\n\
def test_run_rejects_blank():\n\
    assert run(\"   \").ok is False\n";

const PY_CLI: &str = "\"\"\"Tiny CLI for {snake}.\"\"\"\n\
import sys\n\
\n\
from .core import run\n\
\n\
\n\
def main() -> int:\n\
    if len(sys.argv) < 2:\n\
        print(\"usage: {name} <query>\", file=sys.stderr)\n\
        return 2\n\
    r = run(sys.argv[1])\n\
    if not r.ok:\n\
        print(\"no result\", file=sys.stderr)\n\
        return 1\n\
    print(r.value)\n\
    return 0\n";

const PY_ARCHETYPE: Archetype = Archetype {
    kind: "python-pkg",
    description: "a small python package",
    names: &["cinnabar", "vermilion", "ochre", "indigo", "malachite", "citrine"],
    initial: &[
        ("pyproject.toml", PY_PYPROJECT),
        ("src/{snake}/__init__.py", PY_INIT),
        ("src/{snake}/core.py", PY_CORE),
        ("README.md", PY_README),
        (".gitignore", PY_GITIGNORE),
    ],
    patches: &[
        Patch {
            op: PatchOp::Create { path: "src/{snake}/cli.py", body: PY_CLI },
            messages: &["add cli entry point", "scaffold {snake} cli", "wire main() for cli"],
        },
        Patch {
            op: PatchOp::Create { path: "tests/test_core.py", body: PY_TEST },
            messages: &["tests: smoke for run()", "add core tests"],
        },
        Patch {
            op: PatchOp::Append { path: "pyproject.toml", line: "[project.scripts]" },
            messages: &["reserve entry-points table"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Type-annotated, mypy-clean on strict.",
            },
            messages: &["note mypy strict in the readme"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: ".mypy_cache" },
            messages: &["ignore mypy cache"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: ".ruff_cache" },
            messages: &["ignore ruff cache"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: ".pytest_cache" },
            messages: &["ignore pytest cache"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: ".coverage" },
            messages: &["ignore coverage data"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "*.egg-info" },
            messages: &["ignore egg-info"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Tested on CPython 3.11 and 3.12.",
            },
            messages: &["note tested CPython versions"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Source lives at git.bythewood.me/{name}.",
            },
            messages: &["link the source"],
        },
        Patch {
            op: PatchOp::Create {
                path: "src/{snake}/parse.py",
                body: "from __future__ import annotations\n\n\ndef tokenize(s: str) -> list[str]:\n    return [tok for tok in s.split() if tok]\n",
            },
            messages: &["pull tokenize into its own module"],
        },
        Patch {
            op: PatchOp::Create {
                path: "src/{snake}/__main__.py",
                body: "from .cli import main\n\n\nif __name__ == \"__main__\":\n    raise SystemExit(main())\n",
            },
            messages: &["allow `python -m {snake}`"],
        },
        Patch {
            op: PatchOp::Create {
                path: "tests/conftest.py",
                body: "import pytest\n\n\n@pytest.fixture\ndef sample():\n    return \"  Hello  \"\n",
            },
            messages: &["add conftest fixture"],
        },
        Patch {
            op: PatchOp::Create {
                path: "CHANGELOG.md",
                body: "# Changelog\n\n## Unreleased\n\n- Initial slice.\n",
            },
            messages: &["start a changelog"],
        },
        Patch {
            op: PatchOp::Create {
                path: ".python-version",
                body: "3.12\n",
            },
            messages: &["pin python to 3.12"],
        },
        Patch {
            op: PatchOp::Append { path: "CHANGELOG.md", line: "- Add cli entry point." },
            messages: &["changelog: cli entry point"],
        },
        Patch {
            op: PatchOp::Append { path: "CHANGELOG.md", line: "- Tighten Result." },
            messages: &["changelog: Result tightening"],
        },
        Patch {
            op: PatchOp::Append {
                path: "pyproject.toml",
                line: "{snake} = \"{snake}.cli:main\"",
            },
            messages: &["wire console_scripts entry"],
        },
    ],
};

// --- markdown blog ---

const BLOG_README: &str = "# {name}\n\
\n\
Personal notes. Mostly things I want to come back to.\n\
\n\
Posts live in `posts/` as plain markdown, filenames prefixed with the date.\n";

const BLOG_ABOUT: &str = "# About\n\
\n\
This is a small markdown notebook. The build script is whatever static site\n\
generator I'm using this week; the source is just files.\n";

const BLOG_POST_1: &str = "# A note on patience\n\
\n\
The fastest path is rarely the straightest, and the straightest path rarely\n\
the most interesting. Some of the better turns I've taken came from sitting\n\
with a problem long enough to find a third option.\n";

const BLOG_POST_2: &str = "# On reading old code\n\
\n\
Code that has survived a few years tends to teach you something. Not the\n\
\"this is how to write code\" kind of teaching, more like the geology of a\n\
hillside: you can see where things shifted and roughly when.\n";

const BLOG_POST_3: &str = "# Small tools, sharp edges\n\
\n\
The smaller the tool, the more it pays to keep the edge keen. A 200-line\n\
script with crisp behavior outlasts a 2000-line system that almost works.\n";

const BLOG_POST_4: &str = "# Notes from the porch\n\
\n\
Rain since morning. The trees off the porch are picking up a slow, weighted\n\
sound that fits the kind of work I want to do today: quiet, no rush.\n";

const BLOG_GITIGNORE: &str = ".cache\nbuild\n";

const BLOG_ARCHETYPE: Archetype = Archetype {
    kind: "blog",
    description: "personal markdown notebook",
    names: &["backwood-notes", "longshore", "weathersong", "dim-burrows", "ash-and-rime"],
    initial: &[
        ("README.md", BLOG_README),
        ("about.md", BLOG_ABOUT),
        (".gitignore", BLOG_GITIGNORE),
        ("posts/2025-04-12-patience.md", BLOG_POST_1),
    ],
    patches: &[
        Patch {
            op: PatchOp::Create {
                path: "posts/2025-04-19-old-code.md",
                body: BLOG_POST_2,
            },
            messages: &["post: on reading old code"],
        },
        Patch {
            op: PatchOp::Create {
                path: "posts/2025-04-26-small-tools.md",
                body: BLOG_POST_3,
            },
            messages: &["post: small tools, sharp edges"],
        },
        Patch {
            op: PatchOp::Create {
                path: "posts/2025-05-03-porch-notes.md",
                body: BLOG_POST_4,
            },
            messages: &["post: notes from the porch"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Built whenever I have time; please don't link-aggregate.",
            },
            messages: &["readme: ask not to be link-aggregated"],
        },
        Patch {
            op: PatchOp::Append {
                path: "about.md",
                line: "Contact through the address on isaacbythewood.com.",
            },
            messages: &["about: add contact line"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: ".DS_Store" },
            messages: &["stop checking in .DS_Store"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "drafts/" },
            messages: &["ignore the drafts dir"],
        },
        Patch {
            op: PatchOp::Create {
                path: "posts/2025-05-10-quiet-tools.md",
                body: "# Quiet tools\n\nThe tools I keep coming back to all share one trait: they don't try\nto be the center of attention. They wait for instructions, do exactly\nwhat I asked, and get out of the way.\n",
            },
            messages: &["post: quiet tools"],
        },
        Patch {
            op: PatchOp::Create {
                path: "posts/2025-05-17-walking-distance.md",
                body: "# Walking distance\n\nThe radius of my day shrinks when I'm tired. A house, a coffee shop,\na library: the world cooperates with that, if you let it.\n",
            },
            messages: &["post: walking distance"],
        },
        Patch {
            op: PatchOp::Create {
                path: "posts/2025-05-24-stone-light.md",
                body: "# Stone light\n\nLate afternoon light hitting the south wall reads as warm but isn't.\nThe stones have been losing heat since two o'clock; what I'm seeing is\nthe last of it, the way bread smells most strongly when it's already cool.\n",
            },
            messages: &["post: stone light"],
        },
        Patch {
            op: PatchOp::Create {
                path: "feed.xml",
                body: "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n  <title>{name}</title>\n</feed>\n",
            },
            messages: &["scaffold atom feed"],
        },
        Patch {
            op: PatchOp::Create {
                path: "build.sh",
                body: "#!/bin/sh\nset -eu\n# Tiny static-site build. Walks posts/, wraps each in the template.\nfor src in posts/*.md; do\n    echo \"  $(basename \"$src\")\"\ndone\n",
            },
            messages: &["add build.sh"],
        },
        Patch {
            op: PatchOp::Create {
                path: "template.html",
                body: "<!doctype html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><title>{{ title }}</title></head>\n<body>{{ content }}</body>\n</html>\n",
            },
            messages: &["add minimal page template"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "Built with a 40-line shell script. The output is plain HTML.",
            },
            messages: &["readme: note the tiny build"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "_site/" },
            messages: &["ignore the _site output dir"],
        },
        Patch {
            op: PatchOp::Append { path: ".gitignore", line: "node_modules/" },
            messages: &["ignore node_modules (whenever I dabble)"],
        },
    ],
};

// --- dotfiles ---

const DOT_README: &str = "# {name}\n\
\n\
My dotfiles. Bootstrapped via `bin/install`, which symlinks `config/` into\n\
`$HOME`. Tested on macOS and recent Debian.\n";

const DOT_INSTALL: &str = "#!/bin/sh\n\
set -eu\n\
\n\
HERE=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
for src in \"$HERE\"/config/.*; do\n\
    name=$(basename \"$src\")\n\
    case \"$name\" in . | .. ) continue ;; esac\n\
    ln -snf \"$src\" \"$HOME/$name\"\n\
done\n\
echo done.\n";

const DOT_ZSHRC: &str = "# zsh: small + fast. no oh-my-zsh.\n\
\n\
export EDITOR=nvim\n\
export PAGER=less\n\
\n\
HISTFILE=~/.zsh_history\n\
HISTSIZE=10000\n\
SAVEHIST=10000\n\
\n\
setopt SHARE_HISTORY HIST_IGNORE_DUPS INC_APPEND_HISTORY\n\
\n\
alias ll='ls -lah'\n\
alias gs='git status'\n\
alias gd='git diff'\n";

const DOT_TMUX: &str = "# tmux: prefix on ctrl-a, vim keys, no mouse\n\
\n\
unbind C-b\n\
set -g prefix C-a\n\
bind C-a send-prefix\n\
\n\
set -g default-terminal \"tmux-256color\"\n\
set -g escape-time 10\n\
\n\
bind h select-pane -L\n\
bind j select-pane -D\n\
bind k select-pane -U\n\
bind l select-pane -R\n";

const DOT_NVIM: &str = "-- neovim: lean. lazy.nvim handles plugins.\n\
\n\
vim.g.mapleader = ' '\n\
vim.opt.number = true\n\
vim.opt.relativenumber = true\n\
vim.opt.expandtab = true\n\
vim.opt.shiftwidth = 4\n\
vim.opt.tabstop = 4\n\
vim.opt.smartcase = true\n\
\n\
vim.keymap.set('n', '<leader>w', ':write<CR>')\n";

const DOT_GITCONFIG: &str = "[user]\n\
    name = Isaac Bythewood\n\
    email = isaac@bythewood.me\n\
[init]\n\
    defaultBranch = master\n\
[push]\n\
    autoSetupRemote = true\n\
[pull]\n\
    ff = only\n";

const DOT_ARCHETYPE: Archetype = Archetype {
    kind: "dotfiles",
    description: "personal dotfiles",
    names: &["paperhouse", "swale", "hearthstone", "hush-hollow", "gypsum"],
    initial: &[
        ("README.md", DOT_README),
        ("bin/install", DOT_INSTALL),
        ("config/.zshrc", DOT_ZSHRC),
    ],
    patches: &[
        Patch {
            op: PatchOp::Create { path: "config/.tmux.conf", body: DOT_TMUX },
            messages: &[
                "tmux: rebind prefix to ctrl-a",
                "tmux: vim keys for pane nav",
                "add tmux config",
            ],
        },
        Patch {
            op: PatchOp::Create { path: "config/nvim/init.lua", body: DOT_NVIM },
            messages: &[
                "neovim: switch to lazy.nvim",
                "neovim: relative line numbers",
                "first cut of init.lua",
            ],
        },
        Patch {
            op: PatchOp::Create { path: "config/.gitconfig", body: DOT_GITCONFIG },
            messages: &["git: default branch master, push autoSetup", "add gitconfig"],
        },
        Patch {
            op: PatchOp::Append { path: "config/.zshrc", line: "alias gp='git push'" },
            messages: &["zsh: alias gp"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.zshrc",
                line: "alias gpl='git pull --ff-only'",
            },
            messages: &["zsh: alias gpl with ff-only"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.zshrc",
                line: "alias t='tmux attach || tmux'",
            },
            messages: &["zsh: alias t for tmux attach"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.tmux.conf",
                line: "set -g status-style fg=white,bg=default",
            },
            messages: &["tmux: lean status line"],
        },
        Patch {
            op: PatchOp::Append {
                path: "README.md",
                line: "`bin/install` is idempotent: it just refreshes the symlinks.",
            },
            messages: &["readme: note install is idempotent"],
        },
        Patch {
            op: PatchOp::Append { path: "config/.zshrc", line: "alias k='kubectl'" },
            messages: &["zsh: alias k for kubectl"],
        },
        Patch {
            op: PatchOp::Append { path: "config/.zshrc", line: "alias d='docker'" },
            messages: &["zsh: alias d for docker"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.zshrc",
                line: "export FZF_DEFAULT_COMMAND='fd --hidden --follow'",
            },
            messages: &["zsh: faster fzf default command"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.tmux.conf",
                line: "set -g history-limit 50000",
            },
            messages: &["tmux: bigger scrollback"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.tmux.conf",
                line: "set -g renumber-windows on",
            },
            messages: &["tmux: renumber windows on close"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.gitconfig",
                line: "[core]\n    excludesfile = ~/.gitignore_global",
            },
            messages: &["git: global excludes file"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.gitconfig",
                line: "[alias]\n    co = checkout\n    br = branch\n    st = status -sb",
            },
            messages: &["git: short aliases"],
        },
        Patch {
            op: PatchOp::Create {
                path: "config/.gitignore_global",
                body: ".DS_Store\nThumbs.db\n*.swp\n*.swo\n*~\n.idea/\n.vscode/\n",
            },
            messages: &["add global gitignore"],
        },
        Patch {
            op: PatchOp::Create {
                path: "bin/uninstall",
                body: "#!/bin/sh\nset -eu\n# Remove symlinks created by bin/install. Idempotent.\nHERE=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\nfor src in \"$HERE\"/config/.*; do\n    name=$(basename \"$src\")\n    case \"$name\" in . | .. ) continue ;; esac\n    link=\"$HOME/$name\"\n    [ -L \"$link\" ] && rm \"$link\"\ndone\n",
            },
            messages: &["add uninstall script"],
        },
        Patch {
            op: PatchOp::Create {
                path: "config/.editorconfig",
                body: "root = true\n\n[*]\nend_of_line = lf\ninsert_final_newline = true\ntrim_trailing_whitespace = true\nindent_style = space\nindent_size = 4\n",
            },
            messages: &["add editorconfig"],
        },
        Patch {
            op: PatchOp::Create {
                path: "config/starship.toml",
                body: "format = \"$directory$git_branch$git_status$character\"\nadd_newline = false\n\n[character]\nsuccess_symbol = \"[\\u003e](bold green)\"\nerror_symbol = \"[\\u003e](bold red)\"\n",
            },
            messages: &["adopt starship prompt"],
        },
        Patch {
            op: PatchOp::Append {
                path: "config/.zshrc",
                line: "eval \"$(starship init zsh)\"",
            },
            messages: &["zsh: enable starship"],
        },
    ],
};

const ARCHETYPES: &[&Archetype] = &[
    &RUST_ARCHETYPE,
    &TS_ARCHETYPE,
    &PY_ARCHETYPE,
    &BLOG_ARCHETYPE,
    &DOT_ARCHETYPE,
];

// ---------- main ------------------------------------------------------------

struct Opts {
    count: usize,
    days: i64,
    seed: u64,
    reset: bool,
}

fn parse_args() -> Result<Opts, String> {
    let mut opts = Opts {
        count: DEFAULT_COUNT,
        days: DEFAULT_DAYS,
        seed: DEFAULT_SEED,
        reset: false,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--count" => {
                i += 1;
                opts.count = args
                    .get(i)
                    .ok_or("--count needs a value")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?;
            }
            "--days" => {
                i += 1;
                opts.days = args
                    .get(i)
                    .ok_or("--days needs a value")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?;
            }
            "--seed" => {
                i += 1;
                opts.seed = args
                    .get(i)
                    .ok_or("--seed needs a value")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?;
            }
            "--reset" => opts.reset = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown arg: {other}")),
        }
        i += 1;
    }
    Ok(opts)
}

fn print_usage() {
    eprintln!(
        "seed: generate fake bare git repos under {DEST}/\n\
         \n\
         Usage:\n  \
           cargo run --bin seed                       defaults: 8 repos, 30 days, seed 0xC0DEFEED\n  \
           cargo run --bin seed -- --count N          number of repos to generate\n  \
           cargo run --bin seed -- --days D           days of history per repo\n  \
           cargo run --bin seed -- --seed N           PRNG seed (reproducible)\n  \
           cargo run --bin seed -- --reset            wipe {DEST}/ first\n"
    );
}

fn main() {
    if let Err(e) = run() {
        eprintln!("seed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let opts = parse_args()?;

    let dest = PathBuf::from(DEST);
    if opts.reset && dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("reset {DEST}: {e}"))?;
    }
    fs::create_dir_all(&dest).map_err(|e| format!("mkdir {DEST}: {e}"))?;

    let mut rng = Rng::new(opts.seed);
    let chosen = pick_repos(&mut rng, opts.count);

    let now = unix_now();
    let oldest = now - opts.days * 86_400;

    for (arch, name) in &chosen {
        let target = dest.join(format!("{name}.git"));
        if target.exists() {
            println!("  skip   {name} (already in {DEST}/)");
            continue;
        }
        println!("  seed   {name} ({})", arch.kind);
        seed_one(&target, arch, name, oldest, opts.days, &mut rng)
            .map_err(|e| format!("seed {name}: {e}"))?;
    }
    Ok(())
}

/// Pick `count` (archetype, name) pairs, no duplicate names. First pass
/// guarantees one repo per archetype (so the landing page always shows the
/// full variety); any remaining slots are filled by random archetype + name.
fn pick_repos(rng: &mut Rng, count: usize) -> Vec<(&'static Archetype, &'static str)> {
    let mut chosen = Vec::with_capacity(count);
    let mut used: HashSet<&'static str> = HashSet::new();
    let total_names: usize = ARCHETYPES.iter().map(|a| a.names.len()).sum();
    let cap = count.min(total_names);

    // First pass: one per archetype.
    for arch in ARCHETYPES {
        if chosen.len() >= cap {
            break;
        }
        let name = *rng.pick(arch.names);
        if used.insert(name) {
            chosen.push((*arch, name));
        }
    }

    // Fill remaining slots with uniformly random archetype + name.
    let mut guard = 0usize;
    while chosen.len() < cap && guard < cap * 50 {
        let arch = *rng.pick(ARCHETYPES);
        let name = *rng.pick(arch.names);
        if used.insert(name) {
            chosen.push((arch, name));
        }
        guard += 1;
    }
    chosen
}

// ---------- per-repo synthesis ---------------------------------------------

fn seed_one(
    target: &Path,
    arch: &Archetype,
    name: &str,
    oldest: i64,
    days: i64,
    rng: &mut Rng,
) -> Result<(), String> {
    // Build the history in a temp working dir, then bare-clone into the
    // fixtures dir. Doing the bare clone at the end lets us use the regular
    // working-tree commit flow (which is much simpler than driving
    // commit-tree directly).
    let work = std::env::temp_dir().join(format!("repos-seed-{name}"));
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;

    git(&work, &["init", "-q", "-b", "master"])?;

    // Initial files + commit. Always Isaac on the first commit; reads as the
    // "this is the operator's repo" handshake.
    for (path, body) in arch.initial {
        let real_path = expand(path, name);
        let real_body = expand(body, name);
        write_under(&work, &real_path, &real_body).map_err(|e| e.to_string())?;
    }
    git(&work, &["add", "."])?;
    let t = oldest + rng.range(86_400) as i64;
    commit(&work, "initial commit", t, &AUTHORS[0])?;

    // Subsequent commits, spread across the remaining days.
    for day in 1..days {
        // Most days have 0 to 2 commits; ~30% are busy with up to 5.
        let n = if rng.chance(3) {
            1 + rng.range(5)
        } else if rng.chance(6) {
            1 + rng.range(2)
        } else {
            0
        };
        for _ in 0..n {
            // Pick a patch, apply it, check it actually produced a diff. If
            // the patch was a no-op (file already exists with same content,
            // or trailing line is already present), try a different one. Up
            // to a handful of attempts; give up silently if every patch in
            // the pool has already been applied to this repo.
            let mut produced_change = false;
            for _ in 0..6 {
                let patch = rng.pick(arch.patches);
                if !apply_patch(&work, &patch.op, name).map_err(|e| e.to_string())? {
                    continue;
                }
                git(&work, &["add", "."])?;
                if !staged_is_empty(&work)? {
                    // Pick a message from this patch's own pool.
                    let tmpl: &&str = rng.pick(patch.messages);
                    let msg = expand(tmpl, name);
                    let ts = oldest + day * 86_400 + rng.range(86_400) as i64;
                    commit(&work, &msg, ts, pick_author(rng))?;
                    produced_change = true;
                    break;
                }
            }
            // If every retry was a no-op, drop the commit slot rather than
            // emit an empty commit.
            let _ = produced_change;
        }
    }

    // Bare clone into the destination.
    let status = Command::new("git")
        .args(["clone", "--bare", "--quiet"])
        .arg(&work)
        .arg(target)
        .status()
        .map_err(|e| format!("clone: {e}"))?;
    if !status.success() {
        return Err(format!("clone exited {:?}", status.code()));
    }

    // Per-repo description (repos reads `description` for the landing
    // page). git's stock placeholder is filtered out in src/git.rs.
    fs::write(target.join("description"), format!("{}\n", arch.description))
        .map_err(|e| e.to_string())?;

    let _ = fs::remove_dir_all(&work);
    Ok(())
}

/// Apply a patch to the working tree. Returns `Ok(true)` if the disk actually
/// changed, `Ok(false)` if the patch was a no-op for this repo (file already
/// created, line already present). The caller skips committing on `false`.
fn apply_patch(work: &Path, op: &PatchOp, name: &str) -> std::io::Result<bool> {
    match op {
        PatchOp::Create { path, body } => {
            let real_path = expand(path, name);
            let full = work.join(&real_path);
            if full.exists() {
                return Ok(false);
            }
            let real_body = expand(body, name);
            write_under(work, &real_path, &real_body)?;
            Ok(true)
        }
        PatchOp::Append { path, line } => {
            let real_path = expand(path, name);
            let real_line = expand(line, name);
            let full = work.join(&real_path);
            if let Some(p) = full.parent() {
                fs::create_dir_all(p)?;
            }
            // If the line is already present (anywhere in the file), treat
            // this patch as exhausted for this repo. Avoids the README
            // sprouting duplicate sentences when the same Append fires twice.
            if let Ok(existing) = fs::read_to_string(&full) {
                if existing.lines().any(|l| l == real_line) {
                    return Ok(false);
                }
            }
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full)?;
            f.write_all(real_line.as_bytes())?;
            f.write_all(b"\n")?;
            Ok(true)
        }
    }
}

/// True iff `git add .` produced no staged changes.
fn staged_is_empty(work: &Path) -> Result<bool, String> {
    let out = Command::new("git")
        .current_dir(work)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .map_err(|e| format!("git diff --cached: {e}"))?;
    // `git diff --quiet` exits 0 if no diff, 1 if there is one.
    Ok(out.success())
}

fn write_under(work: &Path, rel: &str, body: &str) -> std::io::Result<()> {
    let full = work.join(rel);
    if let Some(p) = full.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(full, body)
}

/// Expand templating placeholders. Two substitutions:
///   `{name}`:  the repo's slug as-is (e.g. `copper-net`).
///   `{snake}`: the same with hyphens turned into underscores. Used inside
///               Rust / Python code blocks where hyphens are illegal as
///               identifiers (`copper-net::version` is wrong;
///               `copper_net::version` is right).
/// Anything else with braces is left untouched, so format-style examples like
/// `println!("{}", x)` in file bodies pass through cleanly.
fn expand(s: &str, name: &str) -> String {
    s.replace("{name}", name)
        .replace("{snake}", &name.replace('-', "_"))
}

// ---------- git plumbing ----------------------------------------------------

fn git(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let s = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .status()
        .map_err(|e| format!("git {args:?}: {e}"))?;
    if !s.success() {
        return Err(format!("git {args:?} exited {:?}", s.code()));
    }
    Ok(())
}

fn commit(cwd: &Path, message: &str, ts: i64, author: &Author) -> Result<(), String> {
    let date = format!("{ts} +0000");
    let s = Command::new("git")
        .current_dir(cwd)
        // Always go through env-vars so author + committer + their dates all
        // agree. `git commit --date=` only sets author date; without the
        // committer envs the commit hash would drift between runs.
        .env("GIT_AUTHOR_NAME", author.name)
        .env("GIT_AUTHOR_EMAIL", author.email)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_NAME", author.name)
        .env("GIT_COMMITTER_EMAIL", author.email)
        .env("GIT_COMMITTER_DATE", &date)
        .args(["commit", "-q", "-m", message])
        .status()
        .map_err(|e| format!("git commit: {e}"))?;
    if !s.success() {
        return Err(format!("git commit exited {:?}", s.code()));
    }
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
