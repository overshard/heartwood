use minijinja::value::Value;
use minijinja::{path_loader, AutoEscape, Environment, Error, ErrorKind, Output, State};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::path::Path;

/// Jinja2-faithful HTML formatter — does NOT escape `/`, so vite asset URLs
/// like `/static/base-abc123.js` come through clean instead of `&#x2f;...`.
fn jinja2_html_formatter(out: &mut Output, state: &State, value: &Value) -> Result<(), Error> {
    if value.is_safe() {
        write!(out, "{value}").map_err(Error::from)?;
        return Ok(());
    }
    let auto_escape = match state.auto_escape() {
        AutoEscape::Html => true,
        AutoEscape::None => false,
        _ => return minijinja::escape_formatter(out, state, value),
    };
    if !auto_escape {
        write!(out, "{value}").map_err(Error::from)?;
        return Ok(());
    }
    if let Some(s) = value.as_str() {
        write_jinja2_html(out, s).map_err(Error::from)?;
    } else if value.is_undefined() || value.is_none() {
        // emit nothing
    } else {
        let stringified = value.to_string();
        write_jinja2_html(out, &stringified).map_err(Error::from)?;
    }
    Ok(())
}

fn write_jinja2_html(out: &mut Output, s: &str) -> std::fmt::Result {
    let mut last = 0;
    for (i, b) in s.bytes().enumerate() {
        let escape = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&#34;",
            b'\'' => "&#39;",
            _ => continue,
        };
        if last < i {
            out.write_str(&s[last..i])?;
        }
        out.write_str(escape)?;
        last = i + 1;
    }
    if last < s.len() {
        out.write_str(&s[last..])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestCtx {
    pub path: String,
}

fn read_manifest(path: &Path) -> JsonValue {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".to_string());
    serde_json::from_str(&text).unwrap_or(JsonValue::Null)
}

fn lookup_asset(manifest: &JsonValue, entry: &str, kind: &str) -> String {
    if let Some(chunk) = manifest.get(entry) {
        if kind == "css" {
            if let Some(css_arr) = chunk.get("css").and_then(|v| v.as_array()) {
                if let Some(first) = css_arr.first().and_then(|v| v.as_str()) {
                    return format!("/static/{first}");
                }
            }
        }
        if let Some(file) = chunk.get("file").and_then(|v| v.as_str()) {
            return format!("/static/{file}");
        }
    }
    format!("/static/{entry}")
}

pub fn build_env(templates_dir: &Path, manifest_path: &Path) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(path_loader(templates_dir));
    env.set_formatter(jinja2_html_formatter);
    // Without this, every {{ ... }} renders raw — minijinja v2 ships no
    // autoescape by default. The default callback enables HTML escape on
    // .html/.htm/.xml, which is what we want for templates and atom feeds.
    env.set_auto_escape_callback(minijinja::default_auto_escape_callback);

    #[cfg(debug_assertions)]
    {
        let path = manifest_path.to_path_buf();
        env.add_function(
            "vite_asset",
            move |entry: String, kind: Option<String>| -> Result<String, Error> {
                let kind = kind.unwrap_or_else(|| "file".to_string());
                let manifest = read_manifest(&path);
                Ok(lookup_asset(&manifest, &entry, &kind))
            },
        );
    }
    #[cfg(not(debug_assertions))]
    {
        let manifest = read_manifest(manifest_path);
        env.add_function(
            "vite_asset",
            move |entry: String, kind: Option<String>| -> Result<String, Error> {
                let kind = kind.unwrap_or_else(|| "file".to_string());
                Ok(lookup_asset(&manifest, &entry, &kind))
            },
        );
    }

    env.add_filter("shortsha", shortsha_filter);
    env.add_filter("naturaltime", naturaltime_filter);
    env.add_filter("rfc3339", rfc3339_filter);
    env.add_filter("filesize", filesize_filter);
    env.add_filter("urlencode", urlencode_filter);
    env.add_filter("urlencode_path", urlencode_path_filter);

    env
}

fn shortsha_filter(value: Value) -> Result<String, Error> {
    let s = value
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| value.to_string());
    Ok(s.chars().take(8).collect())
}

fn rfc3339_filter(value: Value) -> Result<String, Error> {
    let ts = value.as_i64().ok_or_else(|| {
        Error::new(ErrorKind::InvalidOperation, "rfc3339 expects an integer")
    })?;
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0).ok_or_else(|| {
        Error::new(ErrorKind::InvalidOperation, "rfc3339: timestamp out of range")
    })?;
    Ok(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn urlencode_filter(value: Value) -> Result<String, Error> {
    let s = value
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| value.to_string());
    Ok(urlencoding::encode(&s).into_owned())
}

/// Percent-encode each segment of a slash-separated path while keeping the
/// separators, so tree/blob paths with `#`, `?`, or `%` in a filename still
/// produce working hrefs.
fn urlencode_path_filter(value: Value) -> Result<String, Error> {
    let s = value
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| value.to_string());
    Ok(s.split('/')
        .map(|seg| urlencoding::encode(seg).into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

fn filesize_filter(value: Value) -> Result<String, Error> {
    let bytes = value.as_i64().ok_or_else(|| {
        Error::new(ErrorKind::InvalidOperation, "filesize expects an integer")
    })?;
    let b = bytes as f64;
    Ok(if b < 1024.0 {
        format!("{} B", bytes)
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", b / (1024.0 * 1024.0 * 1024.0))
    })
}

/// Render a unix-seconds timestamp as "5 minutes ago" / "3 days ago" / etc.
fn naturaltime_filter(value: Value) -> Result<String, Error> {
    let secs_ago = if let Some(ts) = value.as_i64() {
        chrono::Utc::now().timestamp().saturating_sub(ts)
    } else {
        return Ok(value.to_string());
    };
    if secs_ago < 0 {
        return Ok("in the future".to_string());
    }
    Ok(if secs_ago < 60 {
        "just now".to_string()
    } else if secs_ago < 3600 {
        let m = secs_ago / 60;
        format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
    } else if secs_ago < 86_400 {
        let h = secs_ago / 3600;
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else if secs_ago < 86_400 * 30 {
        let d = secs_ago / 86_400;
        format!("{d} day{} ago", if d == 1 { "" } else { "s" })
    } else if secs_ago < 86_400 * 365 {
        let m = secs_ago / (86_400 * 30);
        format!("{m} month{} ago", if m == 1 { "" } else { "s" })
    } else {
        let y = secs_ago / (86_400 * 365);
        format!("{y} year{} ago", if y == 1 { "" } else { "s" })
    })
}
