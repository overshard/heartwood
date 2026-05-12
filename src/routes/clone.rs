//! Smart-HTTP clone endpoints. Captures any URL whose first segment ends in
//! `.git`, hands the rest off to `git http-backend` as PATH_INFO.

use axum::{
    extract::{ConnectInfo, Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Router,
};
use std::net::SocketAddr;

use crate::http_backend::{self, CgiRequest};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{name_git}/info/refs", get(info_refs))
        .route("/{name_git}/git-upload-pack", post(upload_pack))
        // git push would be /git-receive-pack; we expose it as a 405 so the
        // remote prints a helpful error rather than a generic 404.
        .route("/{name_git}/git-receive-pack", any(receive_pack_forbidden))
}

fn strip_git_suffix(name_git: &str) -> Option<&str> {
    name_git.strip_suffix(".git")
}

fn validate_name(name: &str) -> Result<(), Response> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.starts_with('.')
        || name.contains("..")
    {
        Err((StatusCode::NOT_FOUND, "no such repo").into_response())
    } else {
        Ok(())
    }
}

async fn info_refs(
    Path(name_git): Path<String>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let Some(name) = strip_git_suffix(&name_git) else {
        return (StatusCode::NOT_FOUND, "no such repo").into_response();
    };
    if let Err(r) = validate_name(name) {
        return r;
    }
    // Bare repos live as /srv/git/<name>.git/. We hand the parent dir to
    // git http-backend as GIT_PROJECT_ROOT and set PATH_INFO to the rest.
    let path_info = format!("/{name}.git/info/refs");
    serve(state, req, addr, path_info).await
}

async fn upload_pack(
    Path(name_git): Path<String>,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let Some(name) = strip_git_suffix(&name_git) else {
        return (StatusCode::NOT_FOUND, "no such repo").into_response();
    };
    if let Err(r) = validate_name(name) {
        return r;
    }
    let path_info = format!("/{name}.git/git-upload-pack");
    serve(state, req, addr, path_info).await
}

async fn receive_pack_forbidden() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        "heartwood is read-only; push to the server's git remote directly",
    )
        .into_response()
}

async fn serve(state: AppState, req: Request, addr: SocketAddr, path_info: String) -> Response {
    let (parts, body) = req.into_parts();
    let query = http_backend::extract_query(&parts.uri);
    let content_type = parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let method = parts.method.clone();

    let cgi = CgiRequest {
        repo_root: &state.config.repo_root,
        method,
        path_info,
        query,
        content_type,
        remote_addr: addr.ip().to_string(),
        headers: parts.headers,
    };
    match http_backend::serve(cgi, body).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("http-backend: {e:#}");
            (StatusCode::BAD_GATEWAY, "clone unavailable").into_response()
        }
    }
}

