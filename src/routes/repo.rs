use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::render::{not_found, render};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/{name}", get(repo_page))
}

async fn repo_page(Path(name): Path<String>, State(state): State<AppState>) -> Response {
    let repo_root = state.config.repo_root.clone();
    let clone_base = state.config.clone_base.clone();
    let name_for_blocking = name.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let repo = crate::git::open(&repo_root, &name_for_blocking)?;
        let summary = crate::git::repo_summary(
            &crate::git::resolve_path(&repo_root, &name_for_blocking)?,
            &clone_base,
        )?;
        let head = repo.head_commit().ok();
        let commits = if let Some(h) = head.as_ref() {
            crate::git::recent_commits(&repo, h.id, 10).unwrap_or_default()
        } else {
            Vec::new()
        };
        let readme_html = head.as_ref().and_then(|h| {
            crate::git::read_readme(&repo, h.id).map(|(_, bytes)| {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                crate::markdown::render(&text)
            })
        });
        Ok((summary, commits, readme_html))
    })
    .await;

    let (summary, commits, readme_html) = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return not_found(&state, &name, e),
        Err(e) => {
            tracing::error!("repo_page join: {e}");
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render error")
                .into_response();
        }
    };

    render(
        &state,
        "repo.html",
        &format!("/{name}"),
        minijinja::context! {
            repo => &summary,
            commits => &commits,
            readme_html => readme_html.map(minijinja::Value::from_safe_string),
        },
    )
}
