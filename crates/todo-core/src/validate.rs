//! Whole-file validation built on the parser, plus recursive dependency checks.
//!
//! Line parsing is data-parallel via `rayon`; because the parser is zero-copy
//! the work is trivially `Sync` and needs no locking.

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;

use crate::parse::{self, Issue, ParsedLine, Severity, Span, TokenKind};
use crate::task::Task;
use crate::vocab::Vocab;

/// Validate a todo.txt `source`, using the parser as the primary check and
/// layering vocab and dependency-graph checks on top. `known_done` are the ids
/// known to be completed (from `done.txt`), so they don't read as dangling.
pub fn validate_source(
    _file: &str,
    source: &str,
    known_done: &HashSet<String>,
    vocab: Option<&Vocab>,
) -> Vec<Issue> {
    // 1. Collect physical lines with their file offsets, then parse them in
    //    parallel (each parse borrows its own slice, so no sharing is needed).
    let mut lines: Vec<(&str, usize)> = Vec::new();
    let mut offset = 0usize;
    for raw in source.split_inclusive('\n') {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !line.trim().is_empty() {
            lines.push((line, offset));
        }
        offset += raw.len();
    }

    let results: Vec<(Span, parse::ParseResult<'_>)> = lines
        .par_iter()
        .map(|(line, off)| ((*off)..(*off + line.len()), parse::parse_line(line, *off)))
        .collect();

    let mut issues: Vec<Issue> = Vec::new();
    let mut parsed: Vec<(Span, ParsedLine<'_>)> = Vec::new();
    for (span, result) in results {
        issues.extend(result.issues);
        if let Some(mut line) = result.line {
            // Token spans were line-relative; shift them to file offsets.
            for token in &mut line.tokens {
                token.span.start += span.start;
                token.span.end += span.start;
            }
            parsed.push((span, line));
        }
    }

    // 2. Vocab checks on project/context/status tokens.
    if let Some(vocab) = vocab {
        for (_, line) in &parsed {
            for token in &line.tokens {
                let (axis, value) = match &token.kind {
                    TokenKind::Project(v) => ("module", *v),
                    TokenKind::Context(v) => ("kind", *v),
                    TokenKind::Meta(k, v) if *k == "status" => ("status", *v),
                    _ => continue,
                };
                if !value.is_empty() && !vocab.allowed(axis, value) {
                    issues.push(Issue {
                        span: token.span.clone(),
                        severity: Severity::Warning,
                        message: format!("unknown {axis} value '{}'", token.text),
                    });
                }
            }
        }
    }

    // 3. Dependency existence, duplicates and self-dependencies.
    let mut ids: HashMap<&str, Span> = HashMap::new();
    let mut dupes: Vec<(&str, Span)> = Vec::new();
    for (_, line) in &parsed {
        if let Some(id) = line.id() {
            let span = line
                .tokens
                .iter()
                .find(|t| matches!(&t.kind, TokenKind::Meta(k, _) if *k == "id"))
                .map(|t| t.span.clone())
                .unwrap_or_else(|| line.tokens.first().map(|t| t.span.clone()).unwrap_or(0..0));
            if ids.insert(id, span.clone()).is_some() {
                dupes.push((id, span));
            }
        }
    }
    for (id, span) in &dupes {
        issues.push(Issue {
            span: span.clone(),
            severity: Severity::Error,
            message: format!("duplicate id:{id}"),
        });
    }

    let mut known: HashSet<&str> = ids.keys().copied().collect();
    known.extend(known_done.iter().map(String::as_str));

    for (_, line) in &parsed {
        for token in &line.tokens {
            if let TokenKind::Meta(k, v) = &token.kind {
                if *k != "depends" && *k != "dep" {
                    continue;
                }
                for dep in v.split(',') {
                    if dep.is_empty() || matches!(dep.chars().next(), Some('+' | '@' | '%')) {
                        continue;
                    }
                    if Some(dep) == line.id() {
                        issues.push(Issue {
                            span: token.span.clone(),
                            severity: Severity::Error,
                            message: format!("self-dependency id:{dep}"),
                        });
                    } else if !known.contains(dep) {
                        issues.push(Issue {
                            span: token.span.clone(),
                            severity: Severity::Error,
                            message: format!("depends:{dep} refers to an unknown id"),
                        });
                    }
                }
            }
        }
    }

    // 4. Recursive dependency cycles over fine id links.
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    for (_, line) in &parsed {
        if let Some(id) = line.id() {
            graph.entry(id).or_insert_with(|| {
                line.depends()
                    .into_iter()
                    .filter(|d| !matches!(d.chars().next(), Some('+' | '@' | '%')))
                    .collect()
            });
        }
    }
    let mut id_span: HashMap<&str, Span> = HashMap::new();
    for (line_span, line) in &parsed {
        if let Some(id) = line.id() {
            id_span.entry(id).or_insert_with(|| line_span.clone());
        }
    }
    if let Some(cycle) = find_cycle(&graph) {
        let first = cycle.first().map(String::as_str).unwrap_or("");
        let span = id_span.get(first).cloned().unwrap_or(0..0);
        issues.push(Issue {
            span,
            severity: Severity::Error,
            message: format!("dependency cycle: {}", cycle.join(" -> ")),
        });
    }

    issues
}

/// Validate the tasks already loaded into a [`crate::Store`] by rendering them
/// back to text. Used by plugins that don't keep the raw source.
pub fn validate_store(
    file: &str,
    store: &crate::Store,
    done_ids: &HashSet<String>,
    vocab: Option<&Vocab>,
) -> Vec<Issue> {
    let text = store.render();
    validate_source(file, &text, done_ids, vocab)
}

/// Collect the `id:` values from a set of done tasks.
pub fn done_ids(done: &[Task]) -> HashSet<String> {
    done.iter()
        .filter_map(|t| t.id().map(str::to_string))
        .collect()
}

#[cfg(test)]
fn has(issues: &[Issue], needle: &str) -> bool {
    issues.iter().any(|i| i.message.contains(needle))
}

fn find_cycle<'a>(graph: &HashMap<&'a str, Vec<&'a str>>) -> Option<Vec<String>> {
    let mut visiting: HashSet<&str> = HashSet::new();
    let mut done: HashSet<&str> = HashSet::new();
    for &start in graph.keys() {
        if done.contains(start) {
            continue;
        }
        let mut stack: Vec<&str> = vec![start];
        visiting.insert(start);
        while let Some(&cur) = stack.last() {
            if let Some(next) = graph.get(cur)
                && let Some(unvisited) = next.iter().copied().find(|n| !done.contains(n))
            {
                if visiting.contains(unvisited) {
                    let mut names: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
                    names.push(unvisited.to_string());
                    return Some(names);
                }
                visiting.insert(unvisited);
                stack.push(unvisited);
                continue;
            }
            stack.pop();
            visiting.remove(cur);
            done.insert(cur);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn reports_dangling_and_duplicate() {
        let src = "a id:a depends:b\nb id:a\n";
        let issues = validate_source("t", src, &none(), None);
        assert!(has(&issues, "depends:b refers to an unknown id"));
        assert!(has(&issues, "duplicate id:a"));
    }

    #[test]
    fn reports_cycles() {
        let src = "a id:a depends:b\nb id:b depends:c\nc id:c depends:a\n";
        let issues = validate_source("t", src, &none(), None);
        assert!(has(&issues, "dependency cycle"));
    }

    #[test]
    fn reports_self_dependency() {
        let src = "a id:a depends:a\n";
        let issues = validate_source("t", src, &none(), None);
        assert!(has(&issues, "self-dependency id:a"));
    }

    #[test]
    fn vocab_warning() {
        let mut vocab = crate::vocab::Vocab::default();
        vocab.add("module", &["core".to_string()]);
        let src = "task +core +nope\n";
        let issues = validate_source("t", src, &none(), Some(&vocab));
        assert!(has(&issues, "unknown module value '+nope'"));
    }
}
