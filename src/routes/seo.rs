use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/favicon.ico", get(favicon))
        .route("/favicon.svg", get(favicon))
        .route("/robots.txt", get(robots))
}

/// Serve the favicon from the built `dist/` directory. Vite copies anything in
/// `frontend/static_src/public/` into the build output, so `dist/favicon.svg`
/// is what ships in the runtime image (the `frontend/` source tree is not).
async fn favicon(State(state): State<AppState>) -> Response {
    let full = state.config.root.join("dist/favicon.svg");
    match std::fs::read(&full) {
        Ok(data) => {
            let mut resp = data.into_response();
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, "image/svg+xml".parse().unwrap());
            resp.headers_mut().insert(
                header::CACHE_CONTROL,
                "public, max-age=86400".parse().unwrap(),
            );
            resp
        }
        Err(_) => (StatusCode::NOT_FOUND, "no favicon").into_response(),
    }
}

async fn robots() -> Response {
    let body = "User-agent: *\nAllow: /\n";
    let mut resp = body.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        "text/plain; charset=utf-8".parse().unwrap(),
    );
    resp
}
