use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/{name}/atom.xml", get(feed))
}

async fn feed(Path(name): Path<String>, State(state): State<AppState>) -> Response {
    let repo_root = state.config.repo_root.clone();
    let clone_base = state.config.clone_base.clone();
    let name_for_blocking = name.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let repo = crate::git::open(&repo_root, &name_for_blocking)?;
        let summary = crate::git::repo_summary(
            &crate::git::resolve_path(&repo_root, &name_for_blocking)?,
            &clone_base,
        )?;
        let head = repo.head_commit()?;
        let commits = crate::git::recent_commits(&repo, head.id, 25)?;
        Ok((summary, commits))
    })
    .await;
    let (summary, commits) = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            // Generic body: the error string can carry filesystem paths.
            tracing::debug!("atom feed for unknown repo: {e:#}");
            return (axum::http::StatusCode::NOT_FOUND, "not found").into_response();
        }
        Err(e) => {
            tracing::error!("atom join: {e}");
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "error").into_response();
        }
    };

    let body = match state.env.get_template("atom.xml") {
        Ok(t) => match t.render(minijinja::context! {
            repo => &summary,
            commits => &commits,
            base_url => &state.config.base_url,
            clone_base => &state.config.clone_base,
            now => minijinja::context! { rfc3339 => chrono::Utc::now().to_rfc3339() },
        }) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("atom render: {e}");
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render error")
                    .into_response();
            }
        },
        Err(e) => {
            tracing::error!("atom template: {e}");
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render error")
                .into_response();
        }
    };

    let mut resp = body.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/atom+xml; charset=utf-8".parse().unwrap(),
    );
    resp
}
