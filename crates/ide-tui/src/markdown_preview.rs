//! In-buffer + Canvas markdown rendering, modeled on Obsidian Live Preview +
//! render-markdown.nvim: conceal markup in place (1:1 source lines), pad/align
//! pipe tables, hover-peek one row, and unconceal the whole block under the caret.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
struct TableBlock {
    /// Inclusive start line index in the source.
    start: usize,
    /// Exclusive end line index.
    end: usize,
    sep: usize,
    aligns: Vec<Align>,
    widths: Vec<usize>,
}

/// How a source line is shown while editing / selecting / hovering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RevealOpts {
    /// Mouse hover: peek that one source row (⌜…⌟), Obsidian-style.
    pub hover_line: Option<usize>,
    /// Terminal caret: unconceal the structural block containing this line.
    pub cursor_line: Option<usize>,
    /// Char selection `[start, end)` — any overlapping source line shows raw.
    pub selection: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RevealKind {
    Rendered,
    /// Edit block or selection — plain source, no peek chrome.
    Edit,
    /// Single hovered row with peek markers.
    Hover,
}

/// Canvas preview: may insert virtual border lines (not 1:1 with source).
pub fn render(source: &str, width: usize) -> Text<'static> {
    render_window(source, 0, usize::MAX, width, RevealOpts::default(), true)
}

/// Buffer paint: one display line per source line (render-markdown style).
pub fn render_buffer(
    source: &str,
    first_line: usize,
    line_count: usize,
    width: usize,
    reveal: RevealOpts,
) -> Text<'static> {
    render_window(source, first_line, line_count, width, reveal, false)
}

/// True when `line` is inside the caret's edit block (precise caret columns).
pub fn line_in_edit_block(source: &str, cursor_line: usize, line: usize) -> bool {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    edit_block_range(&lines, cursor_line).is_some_and(|(start, end)| line >= start && line < end)
}

/// True when `line` overlaps a char selection (raw columns match for carets too).
pub fn line_in_selection(source: &str, selection: (usize, usize), line: usize) -> bool {
    let (sel_start, sel_end) = selection;
    if sel_start >= sel_end {
        return false;
    }
    let (line_start, line_end) = line_char_bounds(source, line);
    sel_start < line_end && sel_end > line_start
}

fn line_char_bounds(source: &str, line: usize) -> (usize, usize) {
    let chars: Vec<char> = source.chars().collect();
    let mut line_i = 0usize;
    let mut start = 0usize;
    for (i, ch) in chars.iter().enumerate() {
        if line_i == line {
            start = i;
            break;
        }
        if *ch == '\n' {
            line_i += 1;
            start = i + 1;
        }
    }
    if line_i < line {
        return (chars.len(), chars.len());
    }
    if line_i > line {
        // Empty trailing line after final newline.
        start = chars.len();
    }
    let mut end = start;
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    // Include the newline so a selection through EOL still covers the row.
    if end < chars.len() {
        end += 1;
    }
    (start, end)
}

fn render_window(
    source: &str,
    first_line: usize,
    line_count: usize,
    width: usize,
    reveal: RevealOpts,
    canvas: bool,
) -> Text<'static> {
    let width = width.max(8);
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let tables = find_tables(&lines);
    let edit_range = reveal
        .cursor_line
        .and_then(|line| edit_block_range(&lines, line));
    let end = if line_count == usize::MAX {
        lines.len()
    } else {
        first_line.saturating_add(line_count).min(lines.len())
    };
    let start = first_line.min(lines.len());
    let mut out = Vec::new();
    let mut fenced = false;
    // Track fence state from the start of the file up to the window.
    for line in &lines[..start] {
        let body = trim_nl(line);
        if body.starts_with("```") || body.starts_with("~~~") {
            fenced = !fenced;
        }
    }
    for index in start..end {
        let body = trim_nl(lines[index]);
        let selected = reveal
            .selection
            .is_some_and(|sel| line_in_selection(source, sel, index));
        let kind = reveal_kind(index, edit_range, reveal.hover_line, selected);
        if kind != RevealKind::Rendered {
            let mut line = match kind {
                RevealKind::Hover => reveal_raw_line(body, width),
                RevealKind::Edit => edit_raw_line(body, width),
                RevealKind::Rendered => unreachable!(),
            };
            if selected {
                for span in &mut line.spans {
                    span.style = span.style.add_modifier(Modifier::REVERSED);
                }
            }
            out.push(line);
            if body.starts_with("```") || body.starts_with("~~~") {
                fenced = !fenced;
            }
            continue;
        }
        if body.starts_with("```") || body.starts_with("~~~") {
            fenced = !fenced;
            let lang = body.trim_start_matches(['`', '~']).trim();
            if canvas && !lang.is_empty() && fenced {
                out.push(Line::from(Span::styled(
                    format!(" {} ", lang),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
            } else if !canvas {
                // Conceal fence markers; keep a dim language chip when opening.
                if fenced && !lang.is_empty() {
                    out.push(Line::from(Span::styled(
                        format!(" {} ", lang),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                } else {
                    out.push(Line::from(""));
                }
            } else {
                out.push(Line::from(""));
            }
            continue;
        }
        if fenced {
            out.push(Line::from(Span::styled(
                pad_fit(body, width, Align::Left),
                Style::default().fg(Color::Green),
            )));
            continue;
        }
        if let Some(table) = tables.iter().find(|t| index >= t.start && index < t.end) {
            if canvas && index == table.start {
                out.push(table_border(table, width, BorderKind::Top));
            }
            if index == table.sep {
                out.push(table_border(table, width, BorderKind::Mid));
            } else {
                out.push(render_table_data_row(body, table, width, index == table.start));
            }
            if canvas && index + 1 == table.end {
                out.push(table_border(table, width, BorderKind::Bottom));
            }
            continue;
        }
        out.push(render_plain_line(body, width));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(
            "(empty)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    Text::from(out)
}

fn reveal_kind(
    index: usize,
    edit_range: Option<(usize, usize)>,
    hover_line: Option<usize>,
    selected: bool,
) -> RevealKind {
    if edit_range.is_some_and(|(start, end)| index >= start && index < end) {
        return RevealKind::Edit;
    }
    if selected {
        return RevealKind::Edit;
    }
    if hover_line == Some(index) {
        return RevealKind::Hover;
    }
    RevealKind::Rendered
}

/// Structural block containing `cursor_line`: table, fenced code, or the single line.
fn edit_block_range(lines: &[&str], cursor_line: usize) -> Option<(usize, usize)> {
    if cursor_line >= lines.len() {
        return None;
    }
    if let Some(table) = find_tables(lines)
        .into_iter()
        .find(|t| cursor_line >= t.start && cursor_line < t.end)
    {
        return Some((table.start, table.end));
    }
    if let Some(range) = fence_range(lines, cursor_line) {
        return Some(range);
    }
    Some((cursor_line, cursor_line.saturating_add(1)))
}

fn fence_range(lines: &[&str], cursor_line: usize) -> Option<(usize, usize)> {
    let mut open: Option<(usize, char)> = None;
    for (i, line) in lines.iter().enumerate() {
        let body = trim_nl(line);
        let marker = if body.starts_with("```") {
            Some('`')
        } else if body.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        if let Some(ch) = marker {
            if let Some((start, open_ch)) = open {
                if ch == open_ch {
                    let end = i + 1;
                    if cursor_line >= start && cursor_line < end {
                        return Some((start, end));
                    }
                    open = None;
                }
            } else {
                open = Some((i, ch));
            }
        }
    }
    if let Some((start, _)) = open {
        if cursor_line >= start {
            return Some((start, lines.len()));
        }
    }
    None
}

fn reveal_raw_line(body: &str, width: usize) -> Line<'static> {
    // Hover peek: real source with dim markup + peek chrome.
    let mut spans = Vec::new();
    spans.push(Span::styled(
        "⌜",
        Style::default().fg(Color::Yellow),
    ));
    spans.extend(raw_highlight_spans(body));
    spans.push(Span::styled(
        "⌟",
        Style::default().fg(Color::Yellow),
    ));
    let flat: String = spans.iter().map(|s| s.content.to_string()).collect();
    if flat.chars().count() > width + 2 {
        Line::from(Span::styled(
            format!("⌜{}⌟", truncate(body, width.saturating_sub(2))),
            Style::default().fg(Color::Yellow),
        ))
    } else {
        Line::from(spans)
    }
}

fn edit_raw_line(body: &str, width: usize) -> Line<'static> {
    // Caret block: plain source (no peek chrome) so columns match typing.
    let spans = raw_highlight_spans(body);
    let flat: String = spans.iter().map(|s| s.content.to_string()).collect();
    if flat.chars().count() > width {
        Line::from(Span::styled(
            truncate(body, width),
            Style::default(),
        ))
    } else {
        Line::from(spans)
    }
}

fn raw_highlight_spans(line: &str) -> Vec<Span<'static>> {
    // Lightweight: pipes/hashes/stars dimmed, rest plain.
    let mut spans = Vec::new();
    let mut plain = String::new();
    for ch in line.chars() {
        if matches!(ch, '|' | '#' | '*' | '_' | '`') {
            if !plain.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut plain)));
            }
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            plain.push(ch);
        }
    }
    if !plain.is_empty() {
        spans.push(Span::raw(plain));
    }
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

fn render_plain_line(body: &str, width: usize) -> Line<'static> {
    let trimmed = body.trim_start();
    if trimmed.is_empty() {
        return Line::from("");
    }
    if trimmed.starts_with('#') {
        let level = trimmed.chars().take_while(|ch| *ch == '#').count().clamp(1, 6);
        let title = trimmed[level..].trim_start();
        let icon = match level {
            1 => "󰲡 ",
            2 => "󰲣 ",
            3 => "󰲥 ",
            4 => "󰲧 ",
            5 => "󰲩 ",
            _ => "󰲫 ",
        };
        // Icons may not render in every terminal — fall back to blank indent.
        let prefix = if width > 4 { icon } else { "" };
        let mut spans = vec![Span::styled(
            prefix.to_string(),
            heading_style(level),
        )];
        spans.extend(inline_spans_concealed(title));
        return Line::from(spans);
    }
    if trimmed
        .chars()
        .all(|ch| matches!(ch, '-' | '*' | '_' | ' '))
        && trimmed.chars().filter(|ch| matches!(ch, '-' | '*' | '_')).count() >= 3
    {
        return Line::from(Span::styled(
            "─".repeat(width.min(48)),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if let Some(rest) = list_body(trimmed) {
        let mut spans = vec![Span::styled(
            "• ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )];
        spans.extend(inline_spans_concealed(rest));
        return Line::from(spans);
    }
    if let Some(rest) = trimmed.strip_prefix('>') {
        let mut spans = vec![Span::styled(
            "┃ ",
            Style::default().fg(Color::Yellow),
        )];
        spans.extend(inline_spans_concealed(rest.trim_start()));
        for span in &mut spans[1..] {
            span.style = span
                .style
                .add_modifier(Modifier::ITALIC)
                .fg(Color::Yellow);
        }
        return Line::from(spans);
    }
    Line::from(inline_spans_concealed(trimmed))
}

fn heading_style(level: usize) -> Style {
    match level {
        1 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    }
}

fn list_body(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if matches!(bytes.first(), Some(b'-' | b'*' | b'+')) && bytes.get(1) == Some(&b' ') {
        return Some(line[2..].trim_start());
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && bytes.get(i) == Some(&b'.') && bytes.get(i + 1) == Some(&b' ') {
        return Some(line[i + 2..].trim_start());
    }
    None
}

fn find_tables(lines: &[&str]) -> Vec<TableBlock> {
    let mut tables = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        let a = trim_nl(lines[i]);
        let b = trim_nl(lines[i + 1]);
        if is_table_row(a) && is_table_separator(b) {
            let header_cells = split_cells(a);
            let aligns = parse_aligns(b, header_cells.len());
            let mut end = i + 2;
            while end < lines.len() && is_table_row(trim_nl(lines[end])) {
                end += 1;
            }
            let mut widths = vec![3usize; aligns.len().max(header_cells.len()).max(1)];
            for row_i in i..end {
                if row_i == i + 1 {
                    continue; // separator
                }
                let cells = split_cells(trim_nl(lines[row_i]));
                for (col, cell) in cells.iter().enumerate() {
                    if col >= widths.len() {
                        widths.resize(col + 1, 3);
                    }
                    let w = visible_width(cell).max(1);
                    widths[col] = widths[col].max(w).max(3);
                }
            }
            if aligns.len() < widths.len() {
                // pad aligns
            }
            let mut aligns = aligns;
            while aligns.len() < widths.len() {
                aligns.push(Align::Left);
            }
            tables.push(TableBlock {
                start: i,
                end,
                sep: i + 1,
                aligns,
                widths,
            });
            i = end;
            continue;
        }
        i += 1;
    }
    tables
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.matches('|').count() >= 2
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let inner = t.trim_matches('|');
    !inner.is_empty()
        && inner
            .chars()
            .all(|ch| matches!(ch, '-' | ':' | '|' | ' '))
        && inner.contains('-')
}

fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn parse_aligns(sep: &str, expect: usize) -> Vec<Align> {
    let mut aligns: Vec<Align> = split_cells(sep)
        .into_iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            match (left, right) {
                (true, true) => Align::Center,
                (false, true) => Align::Right,
                _ => Align::Left,
            }
        })
        .collect();
    while aligns.len() < expect {
        aligns.push(Align::Left);
    }
    aligns
}

fn visible_width(cell: &str) -> usize {
    // After concealing ** / * / `, approximate display width.
    let flat = conceal_inline_text(cell);
    flat.chars().count()
}

fn conceal_inline_text(line: &str) -> String {
    inline_spans_concealed(line)
        .into_iter()
        .map(|s| s.content.to_string())
        .collect()
}

enum BorderKind {
    Top,
    Mid,
    Bottom,
}

fn table_border(table: &TableBlock, width: usize, kind: BorderKind) -> Line<'static> {
    let (left, mid, right) = match kind {
        BorderKind::Top => ('┌', '┬', '┐'),
        BorderKind::Mid => ('├', '┼', '┤'),
        BorderKind::Bottom => ('└', '┴', '┘'),
    };
    let mut clean = String::new();
    clean.push(left);
    for (i, w) in table.widths.iter().enumerate() {
        if i > 0 {
            clean.push(mid);
        }
        let cell_w = (*w).saturating_add(2);
        match kind {
            BorderKind::Mid => match table.aligns.get(i).copied().unwrap_or(Align::Left) {
                Align::Left => {
                    clean.push('━');
                    clean.push_str(&"─".repeat(cell_w.saturating_sub(1)));
                }
                Align::Right => {
                    clean.push_str(&"─".repeat(cell_w.saturating_sub(1)));
                    clean.push('━');
                }
                Align::Center => {
                    if cell_w <= 1 {
                        clean.push('━');
                    } else {
                        clean.push('━');
                        clean.push_str(&"─".repeat(cell_w.saturating_sub(2)));
                        clean.push('━');
                    }
                }
            },
            _ => {
                clean.push_str(&"─".repeat(cell_w));
            }
        }
    }
    clean.push(right);
    Line::from(Span::styled(
        truncate(&clean, width),
        Style::default().fg(Color::DarkGray),
    ))
}

fn render_table_data_row(
    body: &str,
    table: &TableBlock,
    width: usize,
    header: bool,
) -> Line<'static> {
    let cells = split_cells(body);
    let mut spans = Vec::new();
    spans.push(Span::styled(
        "│",
        Style::default().fg(Color::DarkGray),
    ));
    let cols = table.widths.len().max(1);
    for col in 0..cols {
        let raw = cells.get(col).map(String::as_str).unwrap_or("");
        let align = table.aligns.get(col).copied().unwrap_or(Align::Left);
        let cell_w = table.widths.get(col).copied().unwrap_or(3);
        let concealed = conceal_inline_text(raw);
        let padded = pad_fit(&concealed, cell_w, align);
        spans.push(Span::raw(" "));
        let style = if header {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        // Re-apply inline styles on the padded visible text when possible.
        if raw.contains("**") || raw.contains('*') || raw.contains('`') || raw.contains('_') {
            let mut inner = inline_spans_concealed(raw);
            for span in &mut inner {
                if header {
                    span.style = span.style.add_modifier(Modifier::BOLD).fg(Color::Cyan);
                }
            }
            // Pad around inline spans to cell_w.
            let vis = conceal_inline_text(raw);
            let pad_total = cell_w.saturating_sub(vis.chars().count());
            let (left, right) = match align {
                Align::Left => (0, pad_total),
                Align::Right => (pad_total, 0),
                Align::Center => (pad_total / 2, pad_total - pad_total / 2),
            };
            if left > 0 {
                spans.push(Span::raw(" ".repeat(left)));
            }
            spans.extend(inner);
            if right > 0 {
                spans.push(Span::raw(" ".repeat(right)));
            }
        } else {
            spans.push(Span::styled(padded, style));
        }
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            "│",
            Style::default().fg(Color::DarkGray),
        ));
    }
    let flat: String = spans.iter().map(|s| s.content.to_string()).collect();
    if flat.chars().count() > width {
        Line::from(Span::styled(
            truncate(&flat, width),
            if header {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ))
    } else {
        Line::from(spans)
    }
}

fn pad_fit(text: &str, width: usize, align: Align) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if len >= width {
        return chars.into_iter().take(width).collect();
    }
    let pad = width - len;
    let (left, right) = match align {
        Align::Left => (0, pad),
        Align::Right => (pad, 0),
        Align::Center => (pad / 2, pad - pad / 2),
    };
    format!(
        "{}{}{}",
        " ".repeat(left),
        chars.into_iter().collect::<String>(),
        " ".repeat(right)
    )
}

fn inline_spans_concealed(line: &str) -> Vec<Span<'static>> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    let mut plain = String::new();
    let flush = |plain: &mut String, spans: &mut Vec<Span<'static>>| {
        if !plain.is_empty() {
            spans.push(Span::raw(std::mem::take(plain)));
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(end) = line[i + 1..].find('`') {
                flush(&mut plain, &mut spans);
                spans.push(Span::styled(
                    line[i + 1..i + 1 + end].to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .bg(Color::Rgb(40, 40, 40)),
                ));
                i += end + 2;
                continue;
            }
        }
        if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = line[i + 2..].find("**") {
                flush(&mut plain, &mut spans);
                spans.push(Span::styled(
                    line[i + 2..i + 2 + end].to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                i += end + 4;
                continue;
            }
        }
        if bytes[i] == b'*' || bytes[i] == b'_' {
            let delim = bytes[i] as char;
            if let Some(rel) = line[i + 1..].find(delim) {
                let end = i + 1 + rel;
                if end > i + 1 {
                    flush(&mut plain, &mut spans);
                    spans.push(Span::styled(
                        line[i + 1..end].to_string(),
                        Style::default().add_modifier(Modifier::ITALIC),
                    ));
                    i = end + 1;
                    continue;
                }
            }
        }
        plain.push(line[i..].chars().next().unwrap_or('?'));
        i = match line[i..].chars().next() {
            Some(ch) => i + ch.len_utf8(),
            None => bytes.len(),
        };
    }
    flush(&mut plain, &mut spans);
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

fn trim_nl(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

fn truncate(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_hides_markup_and_pads_tables() {
        let src = "# Welcome\n\n| Key | Action |\n| --- | -----: |\n| `e` | edit |\n";
        let text = render(src, 40);
        let flat: String = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(flat.contains("Welcome"), "{flat}");
        assert!(!flat.contains('#'), "{flat}");
        assert!(flat.contains('│') || flat.contains('┌'), "{flat}");
        assert!(!flat.contains("| Key |"), "{flat}");
        assert!(flat.contains("edit"), "{flat}");
    }

    #[test]
    fn buffer_reveal_shows_raw_pipe_syntax() {
        let src = "| a | b |\n| - | - |\n| 1 | 2 |\n";
        let text = render_buffer(
            src,
            0,
            3,
            40,
            RevealOpts {
                hover_line: Some(0),
                cursor_line: None,
                selection: None,
            },
        );
        let first: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(first.contains('|'), "{first}");
        assert!(first.contains('⌜') || first.contains("a"), "{first}");
        let text2 = render_buffer(src, 0, 3, 40, RevealOpts::default());
        let first2: String = text2.lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(first2.contains('│'), "{first2}");
        assert!(!first2.contains("| a |"), "{first2}");
    }

    #[test]
    fn right_alignment_pads_left() {
        let src = "| n |\n| -: |\n| 1 |\n| 12 |\n";
        let text = render_buffer(src, 2, 2, 20, RevealOpts::default());
        let row1: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        let row2: String = text.lines[1]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(row1.contains('│'), "{row1}");
        // Right-aligned "1" should have more left padding than "12" in the same column.
        let ones = row1.find('1').expect("1");
        let twelves = row2.find('1').expect("12");
        assert!(
            ones > twelves,
            "expected right-align pad: row1={row1:?} row2={row2:?}"
        );
    }

    #[test]
    fn only_hovered_row_reveals_raw() {
        let src = "| a | b |\n| - | - |\n| 1 | 2 |\n";
        let text = render_buffer(
            src,
            0,
            3,
            40,
            RevealOpts {
                hover_line: Some(2),
                cursor_line: None,
                selection: None,
            },
        );
        let header: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        let data: String = text.lines[2]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(header.contains('│'), "header stays rendered: {header}");
        assert!(!header.contains("| a |"), "{header}");
        assert!(data.contains('|'), "hovered data row raw: {data}");
        assert!(data.contains('⌜'), "{data}");
    }

    #[test]
    fn cursor_on_table_unconceals_whole_block_without_peek_chrome() {
        let src = "intro\n| a | b |\n| - | - |\n| 1 | 2 |\nafter\n";
        let text = render_buffer(
            src,
            0,
            5,
            40,
            RevealOpts {
                hover_line: None,
                cursor_line: Some(3), // data row inside table
                selection: None,
            },
        );
        let flat = |i: usize| -> String {
            text.lines[i]
                .spans
                .iter()
                .map(|s| s.content.to_string())
                .collect()
        };
        assert!(flat(0).contains("intro"), "{}", flat(0));
        assert!(!flat(0).contains('│'), "intro stays non-table: {}", flat(0));
        assert!(flat(1).contains("| a |"), "header raw: {}", flat(1));
        assert!(!flat(1).contains('⌜'), "no hover chrome on edit block: {}", flat(1));
        assert!(flat(2).contains('|') && flat(2).contains('-'), "{}", flat(2));
        assert!(flat(3).contains("| 1 |"), "{}", flat(3));
        assert!(flat(4).contains("after"), "{}", flat(4));
        assert!(line_in_edit_block(src, 3, 1));
        assert!(line_in_edit_block(src, 3, 3));
        assert!(!line_in_edit_block(src, 3, 0));
    }

    #[test]
    fn hover_outside_edit_block_still_peeks_one_row() {
        let src = "| a | b |\n| - | - |\n| 1 | 2 |\n\n# Title\n";
        let text = render_buffer(
            src,
            0,
            5,
            40,
            RevealOpts {
                hover_line: Some(4),
                cursor_line: Some(0), // editing the table
                selection: None,
            },
        );
        let title: String = text.lines[4]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(title.contains('#'), "hovered heading raw: {title}");
        assert!(title.contains('⌜'), "{title}");
        let header: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(header.contains("| a |"), "{header}");
        assert!(!header.contains('⌜'), "{header}");
    }

    #[test]
    fn selection_unconceals_overlapping_lines_only() {
        let src = "# Title\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";
        // Select only the heading line (chars of "# Title\n").
        let sel_end = "# Title\n".chars().count();
        let text = render_buffer(
            src,
            0,
            5,
            40,
            RevealOpts {
                hover_line: None,
                cursor_line: None,
                selection: Some((0, sel_end)),
            },
        );
        let heading: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        let table: String = text.lines[2]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(heading.contains('#'), "selected heading raw: {heading}");
        assert!(
            text.lines[0]
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::REVERSED)),
            "selected rows reverse-video"
        );
        assert!(table.contains('│'), "unselected table stays rendered: {table}");
        assert!(!table.contains("| a |"), "{table}");
        assert!(line_in_selection(src, (0, sel_end), 0));
        assert!(!line_in_selection(src, (0, sel_end), 2));
    }

    #[test]
    fn select_all_unconceals_entire_buffer() {
        let src = "# Title\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";
        let end = src.chars().count();
        let text = render_buffer(
            src,
            0,
            5,
            40,
            RevealOpts {
                hover_line: None,
                cursor_line: None,
                selection: Some((0, end)),
            },
        );
        let flat: String = text
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(flat.contains('#'), "{flat}");
        assert!(flat.contains("| a |"), "{flat}");
        assert!(!flat.contains('│'), "no rendered table under select-all: {flat}");
    }
}
