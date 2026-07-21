use std::sync::OnceLock;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::html::{styled_line_to_highlighted_html, IncludeBackground};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

fn syntax_set() -> &'static SyntaxSet {
    static CELL: OnceLock<SyntaxSet> = OnceLock::new();
    CELL.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static CELL: OnceLock<Theme> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut ts = ThemeSet::load_defaults();
        // base16-eighties.dark reads well on the dark palette and ships
        // with syntect's default themes, so no theme files to vendor.
        ts.themes
            .remove("base16-eighties.dark")
            .or_else(|| ts.themes.remove("Solarized (dark)"))
            .unwrap_or_else(|| ts.themes.into_values().next().expect("at least one theme"))
    })
}

/// `filename` is used only to pick the syntax; pass the blob basename.
pub fn highlight(source: &str, filename: &str) -> String {
    let ss = syntax_set();
    let theme = theme();

    // Never find_syntax_for_file here: it opens the named file on the
    // server's filesystem, but `filename` is a blob basename from the URL,
    // not a real local file. Match on the extension (which for files like
    // "Makefile" can be the whole name), then sniff the first line
    // (shebangs, XML declarations) from the blob itself.
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let syntax = ss
        .find_syntax_by_extension(ext)
        .or_else(|| ss.find_syntax_by_extension(filename))
        .or_else(|| source.lines().next().and_then(|l| ss.find_syntax_by_first_line(l)))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut highlighter = syntect::easy::HighlightLines::new(syntax, theme);
    let mut out = String::from("<pre class=\"hl\"><code>");
    for (i, line) in LinesWithEndings::from(source).enumerate() {
        let ranges = highlighter
            .highlight_line(line, ss)
            .unwrap_or_else(|_| vec![(Default::default(), line)]);
        out.push_str(&format!(
            "<span class=\"ln\" data-n=\"{}\"></span>",
            i + 1
        ));
        out.push_str("<span class=\"l\">");
        let html = styled_line_to_highlighted_html(&ranges, IncludeBackground::No)
            .unwrap_or_else(|_| line.to_string());
        out.push_str(&html);
        out.push_str("</span>");
    }
    out.push_str("</code></pre>");
    out
}
