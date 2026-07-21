//! Thin wrappers over `gix` that pre-shape data into the structs templates
//! want. Every function here is synchronous and blocking; routes call them
//! inside `tokio::task::spawn_blocking` if they're touching anything heavier
//! than HEAD metadata.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Suffix used by the bare repos we expose. `/srv/git/foo.git/` is the
/// canonical layout; anything without `.git` is ignored on discovery so a
/// stray directory doesn't surface as a repo.
pub const BARE_SUFFIX: &str = ".git";

/// Maximum blob size we'll load into memory for /blob and /raw views.
/// Anything larger is almost certainly a binary asset best fetched via
/// `git clone`; serving it inline would risk OOMing the container under
/// concurrent requests.
pub const MAX_BLOB_SIZE: u64 = 25 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct RepoSummary {
    pub name: String,
    pub description: String,
    pub default_branch: String,
    pub head_summary: Option<String>,
    pub head_time: Option<i64>,
    pub head_id: Option<String>,
    pub clone_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub message: String,
    pub author: String,
    pub author_email: String,
    pub time: i64,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub kind: &'static str, // "tree" | "blob" | "commit" (submodule) | "link"
    pub mode: String,
    pub size: Option<u64>,
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlobInfo {
    pub data: Vec<u8>,
    pub size: u64,
    pub is_binary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: &'static str, // "added" | "deleted" | "modified" | "renamed"
    pub hunks: Vec<DiffHunk>,
    pub is_binary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    pub kind: &'static str, // "add" | "del" | "ctx" | "hdr"
    pub text: String,
}

/// Walk `root` once and return one entry per `*.git` directory that contains
/// a HEAD ref. Sorted by most-recently-touched HEAD first so the landing page
/// reads "what's been active" without further sorting in the template.
pub fn discover(root: &Path, clone_base: &str) -> Result<Vec<RepoSummary>> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("repo root {}: {}", root.display(), e);
            return Ok(out);
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with(BARE_SUFFIX) {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        match repo_summary(&path, clone_base) {
            Ok(s) => out.push(s),
            Err(e) => tracing::warn!("skipping {}: {:#}", path.display(), e),
        }
    }
    out.sort_by(|a, b| b.head_time.cmp(&a.head_time).then(a.name.cmp(&b.name)));
    Ok(out)
}

fn short_name(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    name.strip_suffix(BARE_SUFFIX).unwrap_or(name).to_string()
}

pub fn open(repo_root: &Path, name: &str) -> Result<gix::Repository> {
    let path = resolve_path(repo_root, name)?;
    Ok(gix::open(&path).with_context(|| format!("open {}", path.display()))?)
}

/// Resolve `name` (the URL slug, e.g. `analytics` or `blog.bythewood.me`)
/// to the on-disk bare repo path. Rejects any name containing a path
/// separator or starting with a dot so a request for `../etc/passwd` can't
/// escape the repo root.
pub fn resolve_path(repo_root: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.starts_with('.')
        || name.contains("..")
    {
        return Err(anyhow!("invalid repo name: {name}"));
    }
    // Append `.git` to the FULL name, not via Path::set_extension — that
    // would clobber the existing extension on names like `blog.bythewood.me`
    // (giving `blog.bythewood.git`, which doesn't exist).
    let p = repo_root.join(format!("{name}.git"));
    if !p.exists() {
        return Err(anyhow!("repo not found: {name}"));
    }
    Ok(p)
}

pub fn repo_summary(path: &Path, clone_base: &str) -> Result<RepoSummary> {
    let name = short_name(path);
    let repo = gix::open(path).with_context(|| format!("open {}", path.display()))?;
    let description = read_description(path);
    let default_branch = read_default_branch(&repo);
    let clone_url = format!(
        "{}/{}.git",
        clone_base.trim_end_matches('/'),
        name
    );

    let head = match repo.head_commit() {
        Ok(c) => Some(c),
        Err(_) => None,
    };
    let (head_summary, head_time, head_id) = if let Some(c) = head {
        let id = c.id().to_string();
        let summary = c
            .message()
            .ok()
            .and_then(|m| Some(m.summary().to_string()))
            .unwrap_or_default();
        let time = c.time().ok().map(|t| t.seconds);
        (Some(summary), time, Some(id))
    } else {
        (None, None, None)
    };

    Ok(RepoSummary {
        name,
        description,
        default_branch,
        head_summary,
        head_time,
        head_id,
        clone_url,
    })
}

/// Read git's per-repo `description` file. The default text shipped by git
/// (`Unnamed repository; edit this file 'description' to name the repository.`)
/// is treated as empty so the landing page doesn't show that boilerplate.
fn read_description(path: &Path) -> String {
    let raw = std::fs::read_to_string(path.join("description")).unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.starts_with("Unnamed repository") {
        String::new()
    } else {
        trimmed.to_string()
    }
}

fn read_default_branch(repo: &gix::Repository) -> String {
    // HEAD is a symbolic ref like `refs/heads/master`; strip the prefix.
    if let Ok(head) = repo.head() {
        if let Some(name) = head.referent_name() {
            let s = name.as_bstr().to_string();
            if let Some(b) = s.strip_prefix("refs/heads/") {
                return b.to_string();
            }
            return s;
        }
    }
    "master".to_string()
}

/// Resolve a revspec (branch name, tag, full or short oid) into a commit oid.
pub fn resolve_rev(repo: &gix::Repository, rev: &str) -> Result<gix::ObjectId> {
    let id = repo
        .rev_parse_single(rev)
        .map_err(|e| anyhow!("revspec {rev}: {e}"))?
        .detach();
    let obj = repo.find_object(id)?;
    let commit = obj.peel_to_kind(gix::object::Kind::Commit)?;
    Ok(commit.id)
}

pub fn commit_info(repo: &gix::Repository, oid: gix::ObjectId) -> Result<CommitInfo> {
    let commit = repo.find_commit(oid)?;
    let id = commit.id().to_string();
    let short_id = id.chars().take(8).collect();
    let msg = commit.message()?;
    let summary = msg.summary().to_string();
    let message = String::from_utf8_lossy(commit.message_raw()?.as_ref()).to_string();
    let sig = commit.author()?;
    let author = sig.name.to_string();
    let author_email = sig.email.to_string();
    let time = sig.time.seconds;
    let parents = commit.parent_ids().map(|p| p.to_string()).collect();
    Ok(CommitInfo {
        id,
        short_id,
        summary,
        message,
        author,
        author_email,
        time,
        parents,
    })
}

pub fn recent_commits(
    repo: &gix::Repository,
    start: gix::ObjectId,
    limit: usize,
) -> Result<Vec<CommitInfo>> {
    let mut out = Vec::with_capacity(limit);
    let walk = repo.rev_walk([start]).all()?;
    for info in walk.take(limit) {
        let info = info?;
        out.push(commit_info(repo, info.id)?);
    }
    Ok(out)
}

/// Walk a tree at `rev` / `path`. `path` is a `/`-joined string ("", "src",
/// "src/routes"), not pre-split.
pub fn list_tree(
    repo: &gix::Repository,
    rev: gix::ObjectId,
    path: &str,
) -> Result<(Vec<TreeEntry>, Vec<String>)> {
    let commit = repo.find_commit(rev)?;
    let mut tree = commit.tree()?;

    let breadcrumb: Vec<String> = path
        .split('/')
        .filter(|p| !p.is_empty())
        .map(|s| s.to_string())
        .collect();
    if !breadcrumb.is_empty() {
        let entry = tree
            .peel_to_entry_by_path(std::path::PathBuf::from(path))?
            .ok_or_else(|| anyhow!("path not found: {path}"))?;
        if !entry.mode().is_tree() {
            return Err(anyhow!("not a tree: {path}"));
        }
        tree = entry.object()?.try_into_tree().map_err(|_| anyhow!("not a tree: {path}"))?;
    }

    let mut entries = Vec::new();
    for entry_ref in tree.iter() {
        let entry_ref = entry_ref?;
        let name = entry_ref.filename().to_string();
        let mode = entry_ref.mode();
        let kind = if mode.is_tree() {
            "tree"
        } else if mode.is_link() {
            "link"
        } else if mode.is_commit() {
            "commit"
        } else {
            "blob"
        };
        let size = if kind == "blob" {
            // Header lookup only: find_object would load every blob's full
            // contents just to report its size in the listing.
            repo.find_header(entry_ref.oid()).ok().map(|h| h.size())
        } else {
            None
        };
        entries.push(TreeEntry {
            name,
            kind,
            mode: format!("{:o}", *mode),
            size,
            id: entry_ref.oid().to_string(),
        });
    }
    // Trees first, then alphabetical within each group.
    entries.sort_by(|a, b| {
        let group = |k| if k == "tree" { 0 } else { 1 };
        group(a.kind)
            .cmp(&group(b.kind))
            .then(a.name.cmp(&b.name))
    });
    Ok((entries, breadcrumb))
}

pub fn read_blob(repo: &gix::Repository, rev: gix::ObjectId, path: &str) -> Result<BlobInfo> {
    let commit = repo.find_commit(rev)?;
    let mut tree = commit.tree()?;
    if path.is_empty() {
        return Err(anyhow!("empty path"));
    }
    let entry = tree
        .peel_to_entry_by_path(std::path::PathBuf::from(path))?
        .ok_or_else(|| anyhow!("path not found: {path}"))?;
    // Peek the object header before loading: try_into_blob() below would
    // otherwise pull the entire blob into memory, so a 500MB asset in a
    // repo could OOM the container under concurrent requests.
    let header = repo.find_header(entry.oid())?;
    if header.size() > MAX_BLOB_SIZE {
        return Err(anyhow!(
            "blob too large to display ({} bytes; cap is {} bytes)",
            header.size(),
            MAX_BLOB_SIZE
        ));
    }
    let blob = entry
        .object()?
        .try_into_blob()
        .map_err(|_| anyhow!("not a blob: {path}"))?;
    let data = blob.data.clone();
    let size = data.len() as u64;
    let is_binary = looks_binary(&data);
    Ok(BlobInfo { data, size, is_binary })
}

/// "Binary" detection mirrors what git itself does: a NUL byte in the first
/// 8KB. Good enough for the file-view branching (text → syntect, binary →
/// download link).
fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(8192).any(|&b| b == 0)
}

/// Find the README at the repo root (case-insensitive, .md / .markdown / .rst
/// / no extension). Returns the bytes if found.
pub fn read_readme(repo: &gix::Repository, rev: gix::ObjectId) -> Option<(String, Vec<u8>)> {
    let commit = repo.find_commit(rev).ok()?;
    let tree = commit.tree().ok()?;
    let mut candidates: Vec<(String, gix::ObjectId)> = Vec::new();
    for e in tree.iter() {
        let Ok(e) = e else { continue };
        if !e.mode().is_blob() {
            continue;
        }
        let name = e.filename().to_string();
        let lower = name.to_ascii_lowercase();
        if lower == "readme"
            || lower == "readme.md"
            || lower == "readme.markdown"
            || lower == "readme.txt"
            || lower == "readme.rst"
        {
            candidates.push((name, e.oid().into()));
        }
    }
    // Prefer .md, then no-extension, then anything else.
    candidates.sort_by_key(|(n, _)| {
        let l = n.to_ascii_lowercase();
        if l.ends_with(".md") || l.ends_with(".markdown") {
            0
        } else if !l.contains('.') {
            1
        } else {
            2
        }
    });
    let (name, oid) = candidates.into_iter().next()?;
    // A README bigger than this is not a README; skip it rather than feed
    // megabytes through markdown + sanitize on every repo page view.
    const README_CAP: u64 = 1024 * 1024;
    if repo.find_header(oid).ok().map(|h| h.size()).unwrap_or(u64::MAX) > README_CAP {
        return None;
    }
    let blob = repo.find_object(oid).ok()?.try_into_blob().ok()?;
    Some((name, blob.data.clone()))
}

/// Render `git show --format= --patch <oid>` for a single commit and parse
/// the unified-diff output into per-file hunks. We shell out instead of
/// driving `gix-diff` directly: git is the canonical implementation of
/// unified diff and it's already on the runtime image (we need it for
/// `git http-backend`), so the marginal cost is one extra fork per commit
/// view, not a new dependency.
pub fn diff_commit(repo_path: &Path, oid: gix::ObjectId) -> Result<Vec<FileDiff>> {
    let output = std::process::Command::new("git")
        .arg("-c")
        // Don't octal-escape non-ASCII filenames; we parse the unified-diff
        // header by string-matching `diff --git a/... b/...` and quoted paths
        // would break that (and the resulting `path` would render as gibberish).
        .arg("core.quotePath=false")
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg("--format=")
        .arg("--patch")
        .arg("--no-color")
        .arg("-M")
        .arg(oid.to_string())
        .output()
        .context("spawn git show")?;
    if !output.status.success() {
        return Err(anyhow!(
            "git show exited {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    // Cap what we parse and render: a generated-file or vendored-tree commit
    // can produce a diff of hundreds of MB, and every byte of it would be
    // parsed, allocated, and templated. Cut at a line boundary.
    const DIFF_OUTPUT_CAP: usize = 2 * 1024 * 1024;
    let text = String::from_utf8_lossy(&output.stdout);
    if text.len() > DIFF_OUTPUT_CAP {
        tracing::warn!("diff for {oid} truncated at {DIFF_OUTPUT_CAP} bytes");
        let mut end = DIFF_OUTPUT_CAP;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let cut = text[..end].rfind('\n').unwrap_or(end);
        return Ok(parse_unified_diff(&text[..cut]));
    }
    Ok(parse_unified_diff(&text))
}

fn parse_unified_diff(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut current_file: Option<FileDiff> = None;
    let mut current_hunk: Option<DiffHunk> = None;

    let flush_hunk = |file: &mut FileDiff, hunk: &mut Option<DiffHunk>| {
        if let Some(h) = hunk.take() {
            file.hunks.push(h);
        }
    };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(mut f) = current_file.take() {
                flush_hunk(&mut f, &mut current_hunk);
                files.push(f);
            }
            // `diff --git a/foo b/foo`: take the b-path (post-rename side).
            let parts: Vec<&str> = rest.split(' ').collect();
            let new_path = parts
                .last()
                .map(|s| s.trim_start_matches("b/").to_string())
                .unwrap_or_default();
            current_file = Some(FileDiff {
                path: new_path,
                old_path: None,
                status: "modified",
                hunks: Vec::new(),
                is_binary: false,
            });
            continue;
        }
        let Some(file) = current_file.as_mut() else { continue };

        if line.starts_with("new file mode") {
            file.status = "added";
        } else if line.starts_with("deleted file mode") {
            file.status = "deleted";
        } else if line.starts_with("rename from ") {
            file.status = "renamed";
            file.old_path = Some(line.trim_start_matches("rename from ").to_string());
        } else if line.starts_with("Binary files ") {
            file.is_binary = true;
        } else if line.starts_with("@@") {
            flush_hunk(file, &mut current_hunk);
            current_hunk = Some(DiffHunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
        } else if let Some(hunk) = current_hunk.as_mut() {
            let (kind, body): (&'static str, &str) = if let Some(rest) = line.strip_prefix('+') {
                if line.starts_with("+++") {
                    continue;
                }
                ("add", rest)
            } else if let Some(rest) = line.strip_prefix('-') {
                if line.starts_with("---") {
                    continue;
                }
                ("del", rest)
            } else if let Some(rest) = line.strip_prefix(' ') {
                ("ctx", rest)
            } else {
                continue;
            };
            hunk.lines.push(DiffLine {
                kind,
                text: body.to_string(),
            });
        }
    }
    if let Some(mut f) = current_file.take() {
        if let Some(h) = current_hunk.take() {
            f.hunks.push(h);
        }
        files.push(f);
    }
    files
}
