//! Overlay rendering — modal panes for split / sessions / perf / live / help.

use crate::tui::style;
use crate::tui::{App, Overlay};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let popup = centered(area, 70, 60);
    f.render_widget(Clear, popup);

    let title = match app.overlay {
        Overlay::Split => "split",
        Overlay::Sessions => "sessions",
        Overlay::Perf => "perf",
        Overlay::Live => "live",
        Overlay::Help => "help",
        Overlay::None => return,
    };

    let inner = style::panel(f, popup, title);

    let lines: Vec<Line> = match app.overlay {
        Overlay::Help => help_lines(),
        _ => vec![
            Line::from(Span::styled(
                format!("{} (range: {})", title, app.range.label),
                style::label(),
            )),
            Line::from(""),
            Line::from(Span::styled("v1 placeholder. Press Esc to return.", style::dim())),
        ],
    };
    f.render_widget(Paragraph::new(lines), inner);
}

fn help_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled("Range dial", style::title())),
        Line::from(vec![
            Span::styled("  ←/→  ", style::key_hint()),
            Span::styled("step preset", style::label()),
        ]),
        Line::from(vec![
            Span::styled("  t y w h a  ", style::key_hint()),
            Span::styled("today / yday / 7d / 24h / all", style::label()),
        ]),
        Line::from(""),
        Line::from(Span::styled("Drill overlays", style::title())),
        Line::from(vec![
            Span::styled("  d  ", style::key_hint()),
            Span::styled("split (driver vs bot turns)", style::label()),
        ]),
        Line::from(vec![
            Span::styled("  s  ", style::key_hint()),
            Span::styled("sessions list", style::label()),
        ]),
        Line::from(vec![
            Span::styled("  p  ", style::key_hint()),
            Span::styled("perf decomposition", style::label()),
        ]),
        Line::from(vec![
            Span::styled("  l  ", style::key_hint()),
            Span::styled("live tail", style::label()),
        ]),
        Line::from(""),
        Line::from(Span::styled("Other", style::title())),
        Line::from(vec![
            Span::styled("  r  ", style::key_hint()),
            Span::styled("force refresh", style::label()),
        ]),
        Line::from(vec![
            Span::styled("  Esc / q  ", style::key_hint()),
            Span::styled("close overlay / quit TUI", style::label()),
        ]),
    ]
}

fn centered(area: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let w = area.width * pct_w / 100;
    let h = area.height * pct_h / 100;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect { x, y, width: w, height: h }
}
