//! Interactive board TUI (ratatui + crossterm).
//!
//! Column layout, scrolling and keybindings mirror the legacy curses board:
//! `q` quit, `r` reload, `h`/`l` (or arrows) move between columns, `j`/`k`
//! between cards, `g`/`G` jump to the ends, `1`-`6`/`Tab` switch dimension and
//! `enter` toggles the task detail line.

use std::cmp::{max, min};
use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};

use todo_core::deps::{DepGraph, State};

use crate::App;
use crate::model::{self, Card, Column, DIMENSIONS, Dimension, fit};

const HINT: &str =
    " q quit · r reload · h/l column · j/k card · g/G ends · 1-6/Tab dimension · enter details ";

/// Run the interactive board until the user quits. The terminal is always
/// restored, even on error.
pub fn run(app: &mut App) -> todo_core::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, app);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    result
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> todo_core::Result<()> {
    loop {
        let graph = DepGraph::new(&app.store, &app.done);
        let cards = model::cards(&graph);
        let columns = model::build_columns(&cards, &graph, app.dim);

        terminal.draw(|f| draw(f, app, &cards, &columns))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        let ncols = columns.len();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('r') => app.reload()?,
            KeyCode::Char('h') | KeyCode::Left => {
                app.col = app.col.saturating_sub(1);
                app.row = 0;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if ncols > 0 {
                    app.col = (app.col + 1).min(ncols - 1);
                }
                app.row = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let last = active_max(&columns, app.col);
                app.row = (app.row + 1).min(last);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.row = app.row.saturating_sub(1);
            }
            KeyCode::Char('g') => app.row = 0,
            KeyCode::Char('G') => app.row = active_max(&columns, app.col),
            KeyCode::Tab | KeyCode::BackTab => {
                let dim = app.dim.next();
                switch(app, dim);
            }
            KeyCode::Char(c @ '1'..='6') => {
                let dim = DIMENSIONS[c as usize - '1' as usize];
                switch(app, dim);
            }
            KeyCode::Enter => app.detail = !app.detail,
            _ => {}
        }
    }
}

fn switch(app: &mut App, dim: Dimension) {
    app.dim = dim;
    app.col = 0;
    app.row = 0;
    app.coff = 0;
    app.detail = false;
}

fn active_max(columns: &[Column], col: usize) -> usize {
    columns
        .get(col)
        .map(|c| c.cards.len().saturating_sub(1))
        .unwrap_or(0)
}

fn draw(f: &mut Frame, app: &mut App, cards: &[Card], columns: &[Column]) {
    let area = f.area();
    let w = area.width as usize;
    let h = area.height as usize;

    let open = cards.len();
    let blocked = cards.iter().filter(|c| c.state == State::Blocked).count();
    let ready = open.saturating_sub(blocked);

    let title = format!(
        " todo board  ·  {}  ·  {} open  ·  {} blocked  ·  {} ready ",
        app.dim.label(),
        open,
        blocked,
        ready
    );
    render_line(
        f,
        Rect::new(0, 0, area.width, 1),
        &fit(&title, w),
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
    );

    if columns.is_empty() {
        if area.width > 3 && area.height > 2 {
            let text = fit("no open tasks", (area.width - 3) as usize);
            render_line(
                f,
                Rect::new(2, 2, area.width - 3, 1),
                &text,
                Style::default().add_modifier(Modifier::DIM),
            );
        }
        render_hint(f, area, &fit(HINT, w.saturating_sub(1)), dim_style());
        return;
    }

    let ncols = columns.len();
    if app.col >= ncols {
        app.col = ncols - 1;
    }
    let colw = ((w as i64 - 3 * (ncols as i64 - 1)) / ncols as i64).clamp(22, 42) as usize;
    let fit_cols = max(1, (w as i64 + 3) / (colw as i64 + 3)) as usize;
    if app.col < app.coff {
        app.coff = app.col;
    }
    if app.col >= app.coff + fit_cols {
        app.coff = app.col - fit_cols + 1;
    }

    let start = app.coff;
    let end = min(start + fit_cols, ncols);
    let visible = &columns[start..end];
    let av = app.col - start;
    let active_len = visible.get(av).map(|c| c.cards.len()).unwrap_or(0);
    if active_len == 0 {
        app.row = 0;
    } else if app.row >= active_len {
        app.row = active_len - 1;
    }

    let body_h = h.saturating_sub(3);
    let sep_h = h.saturating_sub(2);

    for (ci, col) in visible.iter().enumerate() {
        let x = ci * (colw + 3);
        if x >= w {
            break;
        }
        let active = start + ci == app.col;
        let hdr = fit(&format!(" {} ({}) ", col.key, col.cards.len()), colw);
        let style = if active {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        render_line(f, Rect::new(x as u16, 1, colw as u16, 1), &hdr, style);

        if ci > 0 && x >= 2 {
            let text = vec!["│"; sep_h].join("\n");
            render_line(
                f,
                Rect::new((x - 2) as u16, 1, 1, sep_h as u16),
                &text,
                Style::default().add_modifier(Modifier::DIM),
            );
        }

        let off = if active && app.row >= body_h && body_h > 0 {
            app.row - body_h + 1
        } else {
            0
        };
        for r in 0..body_h {
            let idx = off + r;
            if idx >= col.cards.len() {
                break;
            }
            let card = &cards[col.cards[idx]];
            let mut style = Style::default();
            if card.state == State::Blocked {
                style = style.fg(Color::Red);
            }
            if active && idx == app.row {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let text = card.card_text(colw);
            render_line(
                f,
                Rect::new(x as u16, (1 + r) as u16, colw as u16, 1),
                &text,
                style,
            );
        }
    }

    let bottom_w = area.width.saturating_sub(1);
    if app.detail && active_len > 0 {
        let card = &cards[visible[av].cards[app.row]];
        let mut detail = format!(" #{} {} ", card.line_no, card.raw);
        if !card.reasons.is_empty() {
            detail.push_str("⟵ ");
            detail.push_str(&card.reasons.join(" · "));
        }
        let style = if card.state == State::Blocked {
            Style::default().fg(Color::Yellow)
        } else {
            dim_style()
        };
        render_hint(f, area, &fit(&detail, bottom_w as usize), style);
    } else {
        render_hint(f, area, &fit(HINT, bottom_w as usize), dim_style());
    }
}

/// Render the one-line hint/detail at the bottom.
fn render_hint(f: &mut Frame, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let rect = Rect::new(0, area.height - 1, area.width.saturating_sub(1), 1);
    render_line(f, rect, text, style);
}

fn dim_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn render_line(f: &mut Frame, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    f.render_widget(Paragraph::new(text.to_string()).style(style), area);
}
