//! Repo-relative `file:`/`dir:`/`path:` location tokens and OSC-8 links.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    File,
    Dir,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRef {
    pub kind: PathKind,
    pub path: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

/// The first `file:`/`dir:`/`path:` token in a task, if any.
pub fn first_path_token(tokens: impl IntoIterator<Item = impl AsRef<str>>) -> Option<String> {
    for tok in tokens {
        let tok = tok.as_ref();
        if let Some(rest) = tok
            .strip_prefix("file:")
            .or_else(|| tok.strip_prefix("dir:"))
            .or_else(|| tok.strip_prefix("path:"))
        {
            let _ = rest;
            return Some(tok.to_string());
        }
    }
    None
}

pub fn parse(token: &str) -> Option<PathRef> {
    let (kind, rest) = if let Some(r) = token.strip_prefix("file:") {
        (PathKind::File, r)
    } else if let Some(r) = token.strip_prefix("dir:") {
        (PathKind::Dir, r)
    } else {
        let r = token.strip_prefix("path:")?;
        (PathKind::Path, r)
    };

    let mut line = None;
    let mut col = None;

    // Accept `path:LINE[:COL]` and `path#LINE[:COL]`.
    let (path, tail) = if let Some((h, t)) = rest.split_once('#') {
        (h.to_string(), Some(t.to_string()))
    } else if let Some((h, t)) = split_line_suffix(rest) {
        (h, Some(t))
    } else {
        (rest.to_string(), None)
    };

    if let Some(tail) = tail {
        let mut it = tail.split(':');
        line = it.next().and_then(|s| s.parse().ok());
        col = it.next().and_then(|s| s.parse().ok());
    }

    Some(PathRef {
        kind,
        path,
        line,
        col,
    })
}

/// If `s` ends in `:<digits>[:<digits>]` and the prefix is non-empty, split it.
fn split_line_suffix(s: &str) -> Option<(String, String)> {
    let mut parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let last = parts.pop()?;
    if last.is_empty() || !last.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let maybe_col = last;
    let maybe_line = parts.last().copied().unwrap_or("");
    if maybe_line.chars().all(|c| c.is_ascii_digit()) && !maybe_line.is_empty() && parts.len() >= 2
    {
        let col = maybe_col.to_string();
        parts.pop();
        let line = maybe_line.to_string();
        let head = parts.join(":");
        (!head.is_empty()).then(|| (head, format!("{line}:{col}")))
    } else {
        let head = parts.join(":");
        (!head.is_empty()).then(|| (head, maybe_col.to_string()))
    }
}

/// Absolute path for a `PathRef`, resolved against `base` (the todo dir).
pub fn absolute(base: &Path, r: &PathRef) -> PathBuf {
    let p = Path::new(&r.path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// The cwd relative to `base`, as `todo.sh`/`dep` do for `--here`.
pub fn cwd_rel(cwd: &Path, base: &Path) -> String {
    if cwd == base {
        ".".to_string()
    } else if let Ok(rel) = cwd.strip_prefix(base) {
        rel.to_string_lossy().into_owned()
    } else {
        cwd.to_string_lossy().into_owned()
    }
}

pub fn matches(token: &str, target: &str, base: &Path) -> bool {
    let Some(r) = parse(token) else {
        return false;
    };
    if target == "." {
        return true;
    }
    let p = &r.path;
    match r.kind {
        PathKind::File => {
            if base.join(target).is_dir() {
                p.starts_with(&format!("{target}/"))
            } else {
                p == target
            }
        }
        PathKind::Dir | PathKind::Path => {
            p == target
                || p.starts_with(&format!("{target}/"))
                || target.starts_with(&format!("{p}/"))
        }
    }
}

pub fn line_matches_path<'a>(
    tokens: impl IntoIterator<Item = &'a str>,
    target: &str,
    base: &Path,
) -> bool {
    tokens.into_iter().any(|t| {
        (t.starts_with("file:") || t.starts_with("dir:") || t.starts_with("path:"))
            && matches(t, target, base)
    })
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'%' => out.push_str("%25"),
            b' ' => out.push_str("%20"),
            b'#' => out.push_str("%23"),
            _ => out.push(b as char),
        }
    }
    out
}

/// Render `{abs}`, `{line}`, `{col}` in a link template. When the template has
/// no placeholders the line/col become `#L`/`:C` suffixes or query params.
pub fn build_url(template: &str, abs: &Path, line: Option<u32>, col: Option<u32>) -> String {
    let abs = urlencode(&abs.to_string_lossy());
    let mut url = template.replace("{abs}", &abs);
    if let Some(line) = line
        && !template.contains("{line}")
    {
        if template.contains('?') {
            url = format!("{url}&line={line}");
        } else {
            url = format!("{url}#L{line}");
        }
    }
    if let (Some(_), Some(col)) = (line, col)
        && !template.contains("{col}")
    {
        if template.contains('?') {
            url = format!("{url}&col={col}");
        } else {
            url = format!("{url}:C{col}");
        }
    }
    url = url.replace("{line}", &line.map(|l| l.to_string()).unwrap_or_default());
    url = url.replace("{col}", &col.map(|c| c.to_string()).unwrap_or_default());
    let _ = abs;
    url
}

/// Wrap `text` in an OSC-8 hyperlink when links are enabled.
pub fn osc8(url: &str, text: &str, enabled: bool) -> String {
    if enabled {
        terminal_link::Link::new(text, url).to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_line_and_col() {
        let r = parse("file:src/main.rs:12:5").unwrap();
        assert_eq!(r.kind, PathKind::File);
        assert_eq!(r.path, "src/main.rs");
        assert_eq!(r.line, Some(12));
        assert_eq!(r.col, Some(5));

        let r = parse("dir:src/semantic").unwrap();
        assert_eq!(r.kind, PathKind::Dir);
        assert_eq!(r.path, "src/semantic");
        assert_eq!(r.line, None);

        let r = parse("file:src/a.rs#4").unwrap();
        assert_eq!(r.line, Some(4));
    }
}
