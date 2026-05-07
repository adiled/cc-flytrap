//! Top brand bar + bottom keybar.

use crate::tui::style;
use crate::tui::App;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::time::Instant;

pub fn header(f: &mut Frame, area: Rect, app: &App) {
    let port = app.cfg.port;
    let pid = launchd_pid_from_lsof(port);
    let uptime = uptime_str(app.started);
    let clock = clock_now();

    let left = Line::from(vec![
        Span::styled("▍ ", Style::default().fg(style::CYAN)),
        Span::styled("CCFT", style::brand()),
        Span::styled(" // ", style::dim()),
        Span::styled(env!("CARGO_PKG_VERSION"), style::label()),
        Span::raw("  "),
        Span::styled("ONLINE", style::title()),
    ]);

    let right = Line::from(vec![
        Span::styled("port:", style::dim()),
        Span::styled(port.to_string(), style::value()),
        Span::raw("  "),
        Span::styled("pid:", style::dim()),
        Span::styled(
            if pid > 0 { pid.to_string() } else { "-".into() },
            style::value(),
        ),
        Span::raw("  "),
        Span::styled("up:", style::dim()),
        Span::styled(uptime, style::value()),
        Span::raw("  "),
        Span::styled(clock, style::label()),
    ]);

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    f.render_widget(Paragraph::new(left), layout[0]);
    f.render_widget(
        Paragraph::new(right).alignment(Alignment::Right),
        layout[1],
    );
}

pub fn keybar(f: &mut Frame, area: Rect, _app: &App) {
    use ratatui::layout::{Alignment, Constraint, Direction, Layout};

    // Left half: vim-style command list with leading-char in pink.
    let mut left: Vec<Span> = Vec::new();
    push_vim(&mut left, ":help", 1);
    left.push(gap());
    push_vim(&mut left, ":q quit", 1);
    left.push(gap());
    push_vim(&mut left, ":t today", 1);
    left.push(Span::raw(" "));
    push_vim(&mut left, ":y yday", 1);
    left.push(Span::raw(" "));
    push_vim(&mut left, ":w week", 1);
    left.push(Span::raw(" "));
    push_vim(&mut left, ":a all", 1);
    left.push(gap());
    push_vim(&mut left, ":d split", 1);
    left.push(Span::raw(" "));
    push_vim(&mut left, ":s sessions", 1);
    left.push(Span::raw(" "));
    push_vim(&mut left, ":p perf", 1);
    left.push(Span::raw(" "));
    push_vim(&mut left, ":l live", 1);
    left.push(gap());
    push_vim(&mut left, ":/ search", 1);
    left.push(Span::raw(" "));
    push_vim(&mut left, ":! filter", 1);

    // Right half: scroll/range hints (no pink — they're modal hints).
    let right: Vec<Span> = vec![
        Span::styled("←/→ range", style::dim()),
        Span::raw("  "),
        Span::styled("+/− zoom", style::dim()),
        Span::raw("  "),
        Span::styled("g top  G bottom", style::dim()),
    ];

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);
    f.render_widget(Paragraph::new(Line::from(left)), layout[0]);
    f.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        layout[1],
    );
}

/// Vim-style keybar entry: pink first `accent_count` chars (e.g. ':q' or ':!')
/// followed by dim rest (e.g. ' quit' or ' filter').
fn push_vim(spans: &mut Vec<Span<'static>>, full: &'static str, accent_count: usize) {
    let chars: Vec<char> = full.chars().collect();
    let acc: String = chars.iter().take(accent_count + 1).collect(); // include the `:`
    let rest: String = chars.iter().skip(accent_count + 1).collect();
    spans.push(Span::styled(acc, style::key_hint()));
    if !rest.is_empty() {
        spans.push(Span::styled(rest, style::dim()));
    }
}

fn gap() -> Span<'static> {
    Span::styled("   ", style::dim())
}

fn uptime_str(started: Instant) -> String {
    let s = started.elapsed().as_secs();
    if s < 60 {
        format!("{}s", s)
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        let h = s / 3600;
        let m = (s % 3600) / 60;
        format!("{}h{:02}m", h, m)
    }
}

fn clock_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let dt = time::OffsetDateTime::from_unix_timestamp(now)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let local = dt.to_offset(crate::tui::style::local_offset());
    format!("{:02}:{:02}:{:02}", local.hour(), local.minute(), local.second())
}

fn launchd_pid_from_lsof(port: u16) -> u32 {
    use std::process::Command;
    let out = Command::new("lsof")
        .args(["-t", "-nP", &format!("-iTCP:{}", port), "-sTCP:LISTEN"])
        .output();
    if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout);
        if let Some(line) = s.lines().next() {
            if let Ok(n) = line.trim().parse::<u32>() {
                return n;
            }
        }
    }
    0
}
