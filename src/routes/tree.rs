use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::render::{not_found, render};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{name}/tree/{rev}", get(tree_root))
        .route("/{name}/tree/{rev}/{*path}", get(tree_nested))
}

async fn tree_root(
    Path((name, rev)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    tree_inner(state, name, rev, String::new()).await
}

async fn tree_nested(
    Path((name, rev, path)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> Response {
    tree_inner(state, name, rev, path).await
}

async fn tree_inner(state: AppState, name: String, rev: String, path: String) -> Response {
    let repo_root = state.config.repo_root.clone();
    let clone_base = state.config.clone_base.clone();
    let name_for_blocking = name.clone();
    let rev_for_blocking = rev.clone();
    let path_for_blocking = path.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let repo = crate::git::open(&repo_root, &name_for_blocking)?;
        let summary = crate::git::repo_summary(
            &crate::git::resolve_path(&repo_root, &name_for_blocking)?,
            &clone_base,
        )?;
        let oid = crate::git::resolve_rev(&repo, &rev_for_blocking)?;
        let (entries, breadcrumb) = crate::git::list_tree(&repo, oid, &path_for_blocking)?;
        Ok((summary, entries, breadcrumb))
    })
    .await;
    match result {
        Ok(Ok((summary, entries, breadcrumb))) => render(
            &state,
            "tree.html",
            &format!("/{name}/tree/{rev}/{path}"),
            minijinja::context! {
                repo => &summary,
                rev => &rev,
                path => &path,
                entries => &entries,
                breadcrumb => &breadcrumb,
            },
        ),
        Ok(Err(e)) => not_found(&state, &name, e),
        Err(e) => {
            tracing::error!("tree_page join: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render error").into_response()
        }
    }
}
