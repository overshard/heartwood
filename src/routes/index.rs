use axum::{extract::State, response::Response, routing::get, Router};

use crate::render::render;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(index))
}

async fn index(State(state): State<AppState>) -> Response {
    let repo_root = state.config.repo_root.clone();
    let clone_base = state.config.clone_base.clone();
    let repos = tokio::task::spawn_blocking(move || crate::git::discover(&repo_root, &clone_base))
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    render(
        &state,
        "index.html",
        "/",
        minijinja::context! { repos => repos },
    )
}
