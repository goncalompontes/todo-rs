//! Plain-text snapshot renderer, equivalent to `board --snapshot`.
//!
//! Output is suitable for piping: no cursor movement, and colour only when the
//! caller asks for it (a terminal, no `--no-color`, no `NO_COLOR`).

use std::cmp::{max, min};

use todo_core::deps::State;
use todo_core::style::{self, Palette};

use crate::model::{Card, Column, fit};

/// Render the board to a string, one line per output line.
pub fn render(cards: &[Card], columns: &[Column], width: i64, colors: Palette) -> String {
    if columns.is_empty() {
        return "no open tasks\n".to_string();
    }

    let ncols = columns.len() as i64;
    let raw = (width - 3 * (ncols - 1)) / ncols;
    let colw = raw.clamp(20, 40) as usize;
    let fit_count = max(1, (width + 3) / (colw as i64 + 3)) as usize;
    let shown = &columns[..min(fit_count, columns.len())];

    let sep = format!(" {}│{} ", colors.prefix(style::dim()), colors.reset());
    let mut out = String::new();

    // Header row.
    let headers: Vec<String> = shown
        .iter()
        .map(|col| {
            let label = format!(" {} ({}) ", col.key, col.cards.len());
            format!(
                "{}{}{}",
                colors.prefix(style::bold()),
                fit(&label, colw),
                colors.reset()
            )
        })
        .collect();
    out.push_str(&headers.join(&sep));
    out.push('\n');

    // Card rows.
    let rows = shown.iter().map(|c| c.cards.len()).max().unwrap_or(0);
    for r in 0..rows {
        let mut cells: Vec<String> = Vec::with_capacity(shown.len());
        for col in shown {
            if r < col.cards.len() {
                let card = &cards[col.cards[r]];
                let text = card.card_text(colw);
                if card.state == State::Blocked {
                    cells.push(format!(
                        "{}{}{}",
                        colors.prefix(style::red()),
                        text,
                        colors.reset()
                    ));
                } else {
                    cells.push(format!("{}{}{}", colors.reset(), text, colors.reset()));
                }
            } else {
                cells.push(" ".repeat(colw));
            }
        }
        out.push_str(&cells.join(&sep));
        out.push('\n');
    }

    if columns.len() > shown.len() {
        let more = columns.len() - shown.len();
        out.push_str(&format!(
            "{}… {} more column(s); widen the terminal or use the interactive view{}",
            colors.prefix(style::dim()),
            more,
            colors.reset()
        ));
        out.push('\n');
    }

    out
}
