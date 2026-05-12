use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::render::{not_found, render};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{name}/blob/{rev}/{*path}", get(blob_page))
        .route("/{name}/raw/{rev}/{*path}", get(blob_raw))
}

async fn blob_page(
    Path((name, rev, path)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> Response {
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
        let blob = crate::git::read_blob(&repo, oid, &path_for_blocking)?;
        Ok((summary, blob))
    })
    .await;
    match result {
        Ok(Ok((summary, blob))) => {
            let basename = path.rsplit('/').next().unwrap_or(&path).to_string();
            let highlighted = if blob.is_binary {
                None
            } else {
                let text = String::from_utf8_lossy(&blob.data).into_owned();
                Some(crate::highlight::highlight(&text, &basename))
            };
            render(
                &state,
                "blob.html",
                &format!("/{name}/blob/{rev}/{path}"),
                minijinja::context! {
                    repo => &summary,
                    rev => &rev,
                    path => &path,
                    basename => basename,
                    size => blob.size,
                    is_binary => blob.is_binary,
                    highlighted => highlighted.map(minijinja::Value::from_safe_string),
                },
            )
        }
        Ok(Err(e)) => not_found(&state, &name, e),
        Err(e) => {
            tracing::error!("blob_page join: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "render error").into_response()
        }
    }
}

async fn blob_raw(
    Path((name, rev, path)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> Response {
    let repo_root = state.config.repo_root.clone();
    let name_for_blocking = name.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let repo = crate::git::open(&repo_root, &name_for_blocking)?;
        let oid = crate::git::resolve_rev(&repo, &rev)?;
        let blob = crate::git::read_blob(&repo, oid, &path)?;
        let mime = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        Ok((blob.data, mime))
    })
    .await;
    match result {
        Ok(Ok((data, mime))) => {
            let mut resp = data.into_response();
            let value = mime
                .parse()
                .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream"));
            resp.headers_mut().insert(header::CONTENT_TYPE, value);
            resp
        }
        Ok(Err(e)) => not_found(&state, &name, e),
        Err(e) => {
            tracing::error!("blob_raw join: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "error").into_response()
        }
    }
}
