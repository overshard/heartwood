use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::render::{not_found, render};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/{name}/commit/{sha}", get(commit_page))
}

async fn commit_page(
    Path((name, sha)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    let repo_root = state.config.repo_root.clone();
    let clone_base = state.config.clone_base.clone();
    let name_for_blocking = name.clone();
    let sha_for_blocking = sha.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let repo_path = crate::git::resolve_path(&repo_root, &name_for_blocking)?;
        let repo = crate::git::open(&repo_root, &name_for_blocking)?;
        let summary = crate::git::repo_summary(&repo_path, &clone_base)?;
        let oid = crate::git::resolve_rev(&repo, &sha_for_blocking)?;
        let commit = crate::git::commit_info(&repo, oid)?;
        let files = crate::git::diff_commit(&repo_path, oid)?;
        Ok((summary, commit, files))
    })
    .await;
    match result {
        Ok(Ok((summary, commit, files))) => render(
            &state,
            "commit.html",
            &format!("/{name}/commit/{sha}"),
            minijinja::context! {
                repo => &summary,
                commit => &commit,
                files => &files,
            },
        ),
        Ok(Err(e)) => not_found(&state, &name, e),
        Err(e) => {
            tracing::error!("commit_page join: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render error").into_response()
        }
    }
}
