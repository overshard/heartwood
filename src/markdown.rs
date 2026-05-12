//! Markdown rendering for READMEs. pulldown-cmark with tables, footnotes,
//! strikethrough, and task-list extensions enabled (the subset Github lets
//! you use in a README), then run through ammonia so any inline <script>
//! or other dangerous HTML in the markdown source is stripped.

use pulldown_cmark::{Options, Parser};

pub fn render(input: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    let parser = Parser::new_ext(input, opts);
    let mut html = String::with_capacity(input.len());
    pulldown_cmark::html::push_html(&mut html, parser);
    ammonia::clean(&html)
}
