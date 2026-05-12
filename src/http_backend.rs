//! Smart-HTTP clone via a `git http-backend` subprocess.
//!
//! `git http-backend` is the CGI program that ships with git. It expects
//! request data in CGI env vars + stdin, and writes CGI-style output to
//! stdout: a small block of `Key: Value` headers, a blank line, then the
//! response body.
//!
//! We spawn it per request, pump the request body to its stdin, parse the
//! CGI headers off stdout, and stream the rest as the response.

use anyhow::{anyhow, Context, Result};
use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

pub struct CgiRequest<'a> {
    pub repo_root: &'a Path,
    pub method: Method,
    pub path_info: String,
    pub query: String,
    pub content_type: Option<String>,
    pub remote_addr: String,
    pub headers: HeaderMap,
}

pub async fn serve(req: CgiRequest<'_>, body: Body) -> Result<Response> {
    let mut cmd = Command::new("git");
    cmd.arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GIT_PROJECT_ROOT", req.repo_root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REQUEST_METHOD", req.method.as_str())
        .env("PATH_INFO", &req.path_info)
        .env("QUERY_STRING", &req.query)
        .env("REMOTE_ADDR", &req.remote_addr)
        .env("HTTP_HOST", host_header(&req.headers).unwrap_or(""))
        .env("SERVER_PROTOCOL", "HTTP/1.1");

    if let Some(ct) = req.content_type {
        cmd.env("CONTENT_TYPE", ct);
    }
    if let Some(ce) = req.headers.get("content-encoding").and_then(|v| v.to_str().ok()) {
        cmd.env("HTTP_CONTENT_ENCODING", ce);
    }
    if let Some(ua) = req.headers.get("user-agent").and_then(|v| v.to_str().ok()) {
        cmd.env("HTTP_USER_AGENT", ua);
    }
    if let Some(acc) = req.headers.get("accept").and_then(|v| v.to_str().ok()) {
        cmd.env("HTTP_ACCEPT", acc);
    }
    if let Some(gp) = req
        .headers
        .get("git-protocol")
        .and_then(|v| v.to_str().ok())
    {
        // protocol v2 negotiation header; git http-backend needs this in env
        // form to switch protocols.
        cmd.env("HTTP_GIT_PROTOCOL", gp);
    }

    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().context("spawn git http-backend")?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let mut stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;

    // Pump the request body into the child. POST upload-pack bodies can be
    // megabytes; stream chunk by chunk.
    let writer = tokio::spawn(async move {
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if let Err(e) = stdin.write_all(&bytes).await {
                        tracing::warn!("write to git http-backend stdin: {e}");
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!("client body error: {e}");
                    break;
                }
            }
        }
        let _ = stdin.shutdown().await;
    });

    tokio::spawn(async move {
        let mut buf = String::new();
        if stderr.read_to_string(&mut buf).await.is_ok() && !buf.is_empty() {
            tracing::warn!("git http-backend stderr: {}", buf.trim());
        }
    });

    let mut reader = BufReader::new(stdout);
    let (status, headers) = parse_cgi_headers(&mut reader).await?;

    // Whatever's left in `reader` after the blank line is the response body.
    // Stream it back as a byte stream rather than buffering — pack files can
    // be large.
    let stream = async_stream::stream! {
        let mut buf = vec![0u8; 32 * 1024];
        let mut reader = reader;
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => yield Ok::<_, std::io::Error>(Bytes::copy_from_slice(&buf[..n])),
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }
        // Reap the child so we don't leak zombies.
        let _ = child.wait().await;
        let _ = writer.await;
    };

    let mut resp = Response::builder().status(status);
    for (k, v) in &headers {
        resp = resp.header(k, v);
    }
    Ok(resp.body(Body::from_stream(stream))?)
}

fn host_header(h: &HeaderMap) -> Option<&str> {
    h.get("host").and_then(|v| v.to_str().ok())
}

/// Read CGI-style "Key: Value\r\n" header lines until a blank line. The first
/// `Status: NNN reason` line, if present, becomes the HTTP status.
async fn parse_cgi_headers<R: tokio::io::AsyncBufRead + Unpin>(
    r: &mut R,
) -> Result<(StatusCode, Vec<(HeaderName, HeaderValue)>)> {
    let mut status = StatusCode::OK;
    let mut headers = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = r.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let Some((k, v)) = trimmed.split_once(':') else { continue };
        let key = k.trim();
        let val = v.trim();
        if key.eq_ignore_ascii_case("Status") {
            // Form: `Status: 404 Not Found` or `Status: 200`.
            let code: u16 = val.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(200);
            status = StatusCode::from_u16(code).unwrap_or(StatusCode::OK);
            continue;
        }
        let Ok(name) = HeaderName::try_from(key) else { continue };
        let Ok(value) = HeaderValue::try_from(val) else { continue };
        headers.push((name, value));
    }
    Ok((status, headers))
}

pub fn extract_query(uri: &Uri) -> String {
    uri.query().unwrap_or_default().to_string()
}
