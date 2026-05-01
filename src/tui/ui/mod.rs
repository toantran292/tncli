mod overlays;
mod panel;
mod workspace_ui;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::app::App;

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();
    if size.height < 8 || size.width < 20 {
        f.render_widget(Paragraph::new("terminal too small"), size);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // panel
            Constraint::Length(1), // bottom bar
        ])
        .split(size);

    panel::draw_left_panel(f, app, outer[0]);

    let hints: &[(&str, &str)] =
        &[("s","start"),("x","stop"),("r","restart"),("g","git"),("?","help")];

    draw_bottom_bar(f, outer[1], hints);

    overlays::draw_popups(f, app, size);
    workspace_ui::draw_popups(f, app, size);
}

fn draw_bottom_bar(f: &mut Frame, area: Rect, hints: &[(&str, &str)]) {
    let max_w = area.width as usize;
    let mut hint_spans: Vec<Span> = Vec::new();
    let mut used = 0;
    for (key, desc) in hints.iter() {
        let key_part = format!(" {key} ");
        let desc_part = format!(" {desc} ");
        let entry_w = key_part.len() + desc_part.len();
        if used + entry_w > max_w {
            let key_only = format!(" {key} ");
            if used + key_only.len() + 1 > max_w { break; }
            hint_spans.push(Span::styled(key_only, Style::default().bg(Color::White).fg(Color::Black)));
            used += key.len() + 2;
        } else {
            hint_spans.push(Span::styled(key_part, Style::default().bg(Color::White).fg(Color::Black)));
            hint_spans.push(Span::styled(desc_part, Style::default().fg(Color::Yellow)));
            used += entry_w;
        }
    }
    f.render_widget(Paragraph::new(Line::from(hint_spans)), area);
}
