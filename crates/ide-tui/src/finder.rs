//! Highlight the characters a fuzzy or grep match landed on.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn matched_line(
    text: &str,
    positions: &[usize],
    selected: bool,
    width: usize,
) -> Line<'static> {
    let mut marks = vec![false; text.chars().count()];
    for position in positions {
        if let Some(mark) = marks.get_mut(*position) {
            *mark = true;
        }
    }
    paint(text, &marks, selected, width)
}

pub fn ranged_line(
    text: &str,
    start: usize,
    end: usize,
    selected: bool,
    width: usize,
) -> Line<'static> {
    let mut marks = vec![false; text.chars().count()];
    for (index, mark) in marks.iter_mut().enumerate() {
        if index >= start && index < end {
            *mark = true;
        }
    }
    paint(text, &marks, selected, width)
}

fn paint(text: &str, marks: &[bool], selected: bool, width: usize) -> Line<'static> {
    let hit = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let plain = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut current = None;
    for (index, ch) in text.chars().enumerate().take(width) {
        let is_hit = marks.get(index).copied().unwrap_or(false);
        if current != Some(is_hit) {
            push_span(&mut spans, &mut buf, current, hit, plain);
            current = Some(is_hit);
        }
        buf.push(ch);
    }
    push_span(&mut spans, &mut buf, current, hit, plain);
    if spans.is_empty() {
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn push_span(
    spans: &mut Vec<Span<'static>>,
    buf: &mut String,
    hit: Option<bool>,
    hit_style: Style,
    plain: Style,
) {
    if buf.is_empty() {
        return;
    }
    let style = if hit == Some(true) { hit_style } else { plain };
    spans.push(Span::styled(std::mem::take(buf), style));
}
