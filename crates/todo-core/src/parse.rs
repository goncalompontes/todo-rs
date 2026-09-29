//! A `chumsky` parser for todo.txt lines.
//!
//! Parsing is the primary validation mechanism: syntax (done marker, priority,
//! dates, tag shape) is checked here with span-accurate errors, and callers
//! layer semantic checks (vocab, dependency existence/cycles) on top of the
//! resulting token stream.
//!
//! The parsed representation is **zero-copy**: every token and tag borrows
//! directly from the source line, so parsing a file allocates only the token
//! vectors and diagnostics, never the text itself.

use std::ops::Range;

use chumsky::error::Rich;
use chumsky::input::MapExtra;
use chumsky::prelude::*;

use crate::date;

pub type Span = Range<usize>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind<'a> {
    Word,
    Project(&'a str),
    Context(&'a str),
    Group(&'a str),
    Meta(&'a str, &'a str),
}

#[derive(Debug, Clone)]
pub struct ParsedToken<'a> {
    pub span: Span,
    pub text: &'a str,
    pub kind: TokenKind<'a>,
}

#[derive(Debug, Clone)]
pub struct ParsedLine<'a> {
    pub done: bool,
    pub priority: Option<char>,
    pub completion_date: Option<&'a str>,
    pub creation_date: Option<&'a str>,
    pub tokens: Vec<ParsedToken<'a>>,
}

impl<'a> ParsedLine<'a> {
    pub fn id(&self) -> Option<&'a str> {
        self.tag("id")
    }

    pub fn tag(&self, key: &str) -> Option<&'a str> {
        self.tokens.iter().find_map(|t| match &t.kind {
            TokenKind::Meta(k, v) if *k == key => Some(*v),
            _ => None,
        })
    }

    pub fn depends(&self) -> Vec<&'a str> {
        let mut out = Vec::new();
        for t in &self.tokens {
            if let TokenKind::Meta(k, v) = &t.kind
                && (*k == "depends" || *k == "dep")
            {
                out.extend(v.split(',').filter(|s| !s.is_empty()));
            }
        }
        out
    }

    pub fn description(&self) -> String {
        self.tokens
            .iter()
            .map(|t| t.text)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub span: Span,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ParseResult<'a> {
    pub line: Option<ParsedLine<'a>>,
    pub issues: Vec<Issue>,
}

fn classify(s: &str) -> TokenKind<'_> {
    if let Some(v) = s.strip_prefix('+') {
        TokenKind::Project(v)
    } else if let Some(v) = s.strip_prefix('@') {
        TokenKind::Context(v)
    } else if let Some(v) = s.strip_prefix('%') {
        TokenKind::Group(v)
    } else if let Some((k, v)) = s.split_once(':') {
        if k.is_empty() {
            TokenKind::Word
        } else {
            TokenKind::Meta(k, v)
        }
    } else {
        TokenKind::Word
    }
}

fn valid_date(s: &str) -> bool {
    date::parse(s).is_some()
}

fn line_parser<'a>() -> impl Parser<'a, &'a str, ParsedLine<'a>, extra::Err<Rich<'a, char>>> {
    let d4 = any()
        .filter(|c: &char| c.is_ascii_digit())
        .repeated()
        .exactly(4)
        .to_slice();
    let d2 = any()
        .filter(|c: &char| c.is_ascii_digit())
        .repeated()
        .exactly(2)
        .to_slice();
    let date = d4
        .then_ignore(just('-'))
        .then(d2)
        .then_ignore(just('-'))
        .then(d2)
        .to_slice()
        .validate(|s: &'a str, e, emitter| {
            if !valid_date(s) {
                emitter.emit(Rich::custom(e.span(), format!("invalid date '{s}'")));
            }
            s
        });

    let done = just("x ").to(true).or_not().map(|o| o.unwrap_or(false));
    let priority = just('(')
        .ignore_then(any().filter(|c: &char| c.is_ascii_uppercase()))
        .then_ignore(just(')'))
        .then_ignore(just(' '))
        .or_not();
    let dates = date
        .then_ignore(just(' ').or_not())
        .repeated()
        .at_most(2)
        .collect::<Vec<_>>();

    let token = none_of(' ').repeated().at_least(1).to_slice().map_with(
        |s: &'a str, e: &mut MapExtra<'a, '_, &'a str, extra::Err<Rich<'a, char>>>| {
            let span: Span = e.span().into_range();
            ParsedToken {
                span,
                text: s,
                kind: classify(s),
            }
        },
    );
    let tokens = token
        .separated_by(just(' ').repeated().at_least(1))
        .allow_trailing()
        .collect::<Vec<_>>();

    done.then(priority)
        .then(dates)
        .then(tokens)
        .then_ignore(end())
        .map(|(((done, priority), dates), tokens)| {
            let (completion_date, creation_date) = match (done, dates.len()) {
                (true, 2) => (Some(dates[0]), Some(dates[1])),
                (true, 1) => (Some(dates[0]), None),
                (false, 1) => (None, Some(dates[0])),
                _ => (None, None),
            };
            ParsedLine {
                done,
                priority,
                completion_date,
                creation_date,
                tokens,
            }
        })
}

/// Parse a single line. `base` is the byte offset of the line start in the
/// file, used to shift spans so diagnostics can point at the whole file.
pub fn parse_line(input: &str, base: usize) -> ParseResult<'_> {
    let (line, errors) = line_parser().parse(input).into_output_errors();
    let mut issues: Vec<Issue> = errors
        .into_iter()
        .map(|e| Issue {
            span: shift(e.span().into_range(), base),
            severity: Severity::Error,
            message: e.reason().to_string(),
        })
        .collect();

    if let Some(parsed) = &line {
        issues.extend(validate_tokens(parsed, base));
    }

    ParseResult { line, issues }
}

fn shift(mut span: Span, base: usize) -> Span {
    span.start += base;
    span.end += base;
    span
}

/// Semantic validation of individual metadata tokens (values only; vocab and
/// dependency existence live in `validate`).
fn validate_tokens(line: &ParsedLine<'_>, base: usize) -> Vec<Issue> {
    let mut issues = Vec::new();
    for token in &line.tokens {
        let TokenKind::Meta(key, value) = &token.kind else {
            continue;
        };
        let span = shift(token.span.clone(), base);
        match *key {
            "due" | "t" => {
                if !valid_date(value) {
                    issues.push(Issue {
                        span,
                        severity: Severity::Error,
                        message: format!("{key}:{value} is not a valid YYYY-MM-DD date"),
                    });
                }
            }
            "sev" => {
                if value.parse::<u32>().is_err() {
                    issues.push(Issue {
                        span,
                        severity: Severity::Error,
                        message: format!("sev:{value} is not a number"),
                    });
                }
            }
            "id" => {
                if value.is_empty() {
                    issues.push(Issue {
                        span,
                        severity: Severity::Error,
                        message: "id: must not be empty".to_string(),
                    });
                }
            }
            "depends" | "dep" | "p" => {
                if value.is_empty() || value.split(',').any(|p| p.is_empty()) {
                    issues.push(Issue {
                        span,
                        severity: Severity::Error,
                        message: format!("{key}:{value} has an empty id"),
                    });
                }
            }
            "file" | "dir" | "path" => {
                if crate::paths::parse(&format!("{key}:{value}")).is_none() {
                    issues.push(Issue {
                        span,
                        severity: Severity::Error,
                        message: format!("{key}:{value} is not a valid path location"),
                    });
                }
            }
            "status" if value.is_empty() => {
                issues.push(Issue {
                    span,
                    severity: Severity::Error,
                    message: "status: must not be empty".to_string(),
                });
            }
            _ => {}
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parses_plain() {
        let r = parse_line("(A) 2026-09-01 buy milk +shop @errand", 0);
        assert!(r.issues.is_empty());
        let line = r.line.unwrap();
        assert_eq!(line.priority, Some('A'));
        assert_eq!(line.creation_date, Some("2026-09-01"));
        assert_eq!(line.tokens.len(), 4);
    }

    #[test]
    fn rejects_bad_date() {
        let r = parse_line("x 2026-13-01 done", 0);
        assert!(!r.issues.is_empty());
    }

    #[test]
    fn validates_meta() {
        let r = parse_line("task sev:abc due:nope", 0);
        assert_eq!(r.issues.len(), 2);
    }

    proptest! {
        /// Parsing arbitrary input never panics.
        #[test]
        fn parse_never_panics(input in ".{0,200}") {
            let _ = parse_line(&input, 0);
        }

        /// A well-formed task built from parts round-trips its description.
        #[test]
        fn description_round_trips(desc in "[ -~]{0,80}") {
            let line = format!("(A) 2026-09-01 {desc}");
            let result = parse_line(&line, 0);
            if let Some(parsed) = result.line {
                assert_eq!(parsed.description(), desc.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
    }
}
