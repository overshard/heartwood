//! Markdown rendering for READMEs. pulldown-cmark with tables, footnotes,
//! strikethrough, and task-list extensions enabled (the subset Github lets
//! you use in a README), then run through ammonia so any inline <script>
//! or other dangerous HTML in the markdown source is stripped.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};

/// Render a README's markdown to sanitized HTML. Relative link targets are
/// prefixed with `link_base` (typically `/<name>/blob/<branch>`) and relative
/// image targets with `image_base` (typically `/<name>/raw/<branch>`) so they
/// resolve against the repo instead of the page's URL.
pub fn render(input: &str, link_base: &str, image_base: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    let parser = Parser::new_ext(input, opts).map(|event| match event {
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => Event::Start(Tag::Link {
            link_type,
            dest_url: rewrite(&dest_url, link_base),
            title,
            id,
        }),
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => Event::Start(Tag::Image {
            link_type,
            dest_url: rewrite(&dest_url, image_base),
            title,
            id,
        }),
        other => other,
    });
    let mut html = String::with_capacity(input.len());
    pulldown_cmark::html::push_html(&mut html, parser);
    ammonia::clean(&html)
}

/// Prefix a URL with `base` if it's a relative path (i.e. not absolute,
/// protocol-relative, scheme-bearing, or a bare fragment).
fn rewrite<'a>(url: &CowStr<'a>, base: &str) -> CowStr<'a> {
    if url.is_empty()
        || url.starts_with('/')
        || url.starts_with('#')
        || url.starts_with("//")
        || has_scheme(url)
    {
        return url.clone();
    }
    let mut path: &str = url;
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    CowStr::from(format!("{}/{}", base.trim_end_matches('/'), path))
}

/// True if `url` begins with an RFC 3986 scheme (`http:`, `mailto:`, etc.).
fn has_scheme(url: &str) -> bool {
    let bytes = url.as_bytes();
    if bytes.first().is_none_or(|b| !b.is_ascii_alphabetic()) {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        match b {
            b':' => return i > 0,
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'+' | b'.' | b'-' => continue,
            _ => return false,
        }
    }
    false
}
