use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use chrono::Datelike;

use crate::templates::RequestCtx;
use crate::AppState;

pub fn render(
    state: &AppState,
    template: &str,
    path: &str,
    extra: minijinja::Value,
) -> Response {
    let tmpl = match state.env.get_template(template) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("template '{}': {}", template, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "template error").into_response();
        }
    };
    let body = tmpl.render(minijinja::context! {
        request => RequestCtx { path: path.to_string() },
        now => minijinja::context! { year => chrono::Local::now().year() },
        site => minijinja::context! {
            title => &state.config.site_title,
            tagline => &state.config.site_tagline,
            clone_base => &state.config.clone_base,
        },
        base_url => &state.config.base_url,
        ..extra
    });
    match body {
        Ok(s) => Html(s).into_response(),
        Err(e) => {
            tracing::error!("render '{}': {}", template, e);
            (StatusCode::INTERNAL_SERVER_ERROR, "render error").into_response()
        }
    }
}

/// Log the underlying reason, then render the themed 404. Used everywhere
/// instead of returning the raw error string so filesystem paths and gix
/// internals don't leak into the response.
pub fn not_found(state: &AppState, name: &str, reason: impl std::fmt::Display) -> Response {
    tracing::info!("{name}: {reason}");
    let body = render(
        state,
        "not_found.html",
        &format!("/{name}"),
        minijinja::context! { name => name },
    );
    (StatusCode::NOT_FOUND, body).into_response()
}

