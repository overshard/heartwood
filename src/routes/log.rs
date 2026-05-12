use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;

use crate::render::{not_found, render};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/{name}/log", get(log_page))
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    #[serde(default)]
    pub rev: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

async fn log_page(
    Path(name): Path<String>,
    Query(q): Query<LogQuery>,
    State(state): State<AppState>,
) -> Response {
    let repo_root = state.config.repo_root.clone();
    let clone_base = state.config.clone_base.clone();
    let rev = q.rev.clone();
    let limit = q.limit.unwrap_or(100).min(500);
    let name_for_blocking = name.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let repo = crate::git::open(&repo_root, &name_for_blocking)?;
        let summary = crate::git::repo_summary(
            &crate::git::resolve_path(&repo_root, &name_for_blocking)?,
            &clone_base,
        )?;
        let rev_used = rev.unwrap_or_else(|| summary.default_branch.clone());
        let oid = crate::git::resolve_rev(&repo, &rev_used)?;
        let commits = crate::git::recent_commits(&repo, oid, limit)?;
        Ok((summary, rev_used, commits))
    })
    .await;
    match result {
        Ok(Ok((summary, rev_used, commits))) => render(
            &state,
            "log.html",
            &format!("/{name}/log"),
            minijinja::context! {
                repo => &summary,
                rev => rev_used,
                commits => &commits,
            },
        ),
        Ok(Err(e)) => not_found(&state, &name, e),
        Err(e) => {
            tracing::error!("log_page join: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render error").into_response()
        }
    }
}
