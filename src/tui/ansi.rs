use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Parse ANSI line with optional search highlight.
pub fn parse_ansi_line_with_search(text: &str, query: &str, is_current: bool) -> Line<'static> {
    if query.is_empty() {
        return parse_ansi_line(text);
    }

    // First parse the ANSI line into spans
    let line = parse_ansi_line(text);

    // Now apply search highlighting by splitting spans where query matches
    let highlight_style = if is_current {
        Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)
    };

    let query_lower = query.to_lowercase();
    let mut result_spans: Vec<Span<'static>> = Vec::new();

    for span in line.spans {
        let content = span.content.to_string();
        let content_lower = content.to_lowercase();
        let style = span.style;

        let mut last = 0;
        let mut start = 0;
        while start < content_lower.len() {
            if let Some(pos) = content_lower[start..].find(&query_lower) {
                let abs_pos = start + pos;
                // text before match
                if abs_pos > last {
                    result_spans.push(Span::styled(content[last..abs_pos].to_string(), style));
                }
                // matched text
                let end = abs_pos + query_lower.len();
                result_spans.push(Span::styled(content[abs_pos..end].to_string(), highlight_style));
                last = end;
                start = end;
            } else {
                break;
            }
        }
        // remaining text
        if last < content.len() {
            result_spans.push(Span::styled(content[last..].to_string(), style));
        }
    }

    Line::from(result_spans)
}

/// Parse a string containing ANSI escape codes into a ratatui Line.
pub fn parse_ansi_line(text: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut fg: Option<Color> = None;
    let mut bg: Option<Color> = None;
    let mut modifier = Modifier::empty();
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // flush buffer
            if !buf.is_empty() {
                let style = build_style(fg, bg, modifier);
                spans.push(Span::styled(std::mem::take(&mut buf), style));
            }
            // parse CSI sequence
            i += 2;
            let mut params = String::new();
            while i < chars.len() && chars[i] != 'm' {
                params.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip 'm'
            }
            apply_sgr(&params, &mut fg, &mut bg, &mut modifier);
        } else {
            buf.push(chars[i]);
            i += 1;
        }
    }

    if !buf.is_empty() {
        let style = build_style(fg, bg, modifier);
        spans.push(Span::styled(buf, style));
    }

    Line::from(spans)
}

fn build_style(fg: Option<Color>, bg: Option<Color>, modifier: Modifier) -> Style {
    let mut style = Style::default();
    if let Some(c) = fg {
        style = style.fg(c);
    }
    if let Some(c) = bg {
        style = style.bg(c);
    }
    style = style.add_modifier(modifier);
    style
}

fn ansi_to_color(code: u8) -> Option<Color> {
    match code {
        0 => Some(Color::Black),
        1 => Some(Color::Red),
        2 => Some(Color::Green),
        3 => Some(Color::Yellow),
        4 => Some(Color::Blue),
        5 => Some(Color::Magenta),
        6 => Some(Color::Cyan),
        7 => Some(Color::White),
        _ => None,
    }
}

fn apply_sgr(params: &str, fg: &mut Option<Color>, bg: &mut Option<Color>, modifier: &mut Modifier) {
    let codes: Vec<u32> = params
        .split(';')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();

    if codes.is_empty() {
        // ESC[m = reset
        *fg = None;
        *bg = None;
        *modifier = Modifier::empty();
        return;
    }

    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => {
                *fg = None;
                *bg = None;
                *modifier = Modifier::empty();
            }
            1 => *modifier |= Modifier::BOLD,
            2 => *modifier |= Modifier::DIM,
            4 => *modifier |= Modifier::UNDERLINED,
            22 => *modifier -= Modifier::BOLD | Modifier::DIM,
            24 => *modifier -= Modifier::UNDERLINED,
            30..=37 => *fg = ansi_to_color((codes[i] - 30) as u8),
            38 => {
                if i + 1 < codes.len() && codes[i + 1] == 5 && i + 2 < codes.len() {
                    *fg = Some(Color::Indexed(codes[i + 2] as u8));
                    i += 2;
                }
            }
            39 => *fg = None,
            40..=47 => *bg = ansi_to_color((codes[i] - 40) as u8),
            48 => {
                if i + 1 < codes.len() && codes[i + 1] == 5 && i + 2 < codes.len() {
                    *bg = Some(Color::Indexed(codes[i + 2] as u8));
                    i += 2;
                }
            }
            49 => *bg = None,
            90..=97 => {
                *fg = ansi_to_color((codes[i] - 90) as u8);
                *modifier |= Modifier::BOLD;
            }
            100..=107 => {
                *bg = ansi_to_color((codes[i] - 100) as u8);
            }
            _ => {}
        }
        i += 1;
    }
}
