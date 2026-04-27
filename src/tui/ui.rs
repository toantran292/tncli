use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use super::app::{App, Focus, Section};

const LEFT_W: u16 = 28;

pub fn draw(f: &mut Frame, app: &mut App) {
    if app.copy_mode {
        draw_copy_mode(f, app);
        return;
    }

    let size = f.area();
    if size.height < 10 || size.width < 60 {
        f.render_widget(Paragraph::new("terminal too small (min 60x10)"), size);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(size);

    // Header
    let title = format!(" tncli — {} ", app.session);
    f.render_widget(
        Paragraph::new(title)
            .style(Style::default().bg(Color::White).fg(Color::Black).add_modifier(Modifier::BOLD))
            .alignment(ratatui::layout::Alignment::Center),
        outer[0],
    );

    // Panels
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_W), Constraint::Min(10)])
        .split(outer[2]);

    let left_area = panels[0];
    let right_area = panels[1];

    draw_left_panel(f, app, left_area);
    draw_log_panel(f, app, right_area);

    // Key hints
    let hints = if app.interactive_mode {
        &[("Esc","exit interactive"),("type","send to pane")][..]
    } else if app.focus == Focus::Left {
        &[("enter","toggle"),("s","start"),("x","stop"),("r","restart"),("X","stop all"),("l/tab","logs"),("a","attach"),("q","quit")][..]
    } else {
        &[("j/k","scroll"),("G","bottom"),("g","top"),("/","search"),("i","interact"),("h/tab","back"),("n/N","cycle"),("y","copy"),("q","quit")][..]
    };

    let hint_spans: Vec<Span> = hints.iter().flat_map(|(key, desc)| vec![
        Span::styled(format!(" {key} "), Style::default().bg(Color::White).fg(Color::Black)),
        Span::styled(format!(" {desc} "), Style::default().fg(Color::Yellow)),
    ]).collect();
    f.render_widget(Paragraph::new(Line::from(hint_spans)), outer[3]);

    // Status bar
    draw_status_bar(f, app, outer[4]);
}

fn draw_left_panel(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let svc_h = app.services.len() as u16 + 2;
    let combo_h = app.combos.len() as u16 + 2;

    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(svc_h), Constraint::Length(combo_h), Constraint::Min(0)])
        .split(area);

    let border = if app.focus == Focus::Left {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Services
    let items: Vec<ListItem> = app.services.iter().enumerate().map(|(i, svc)| {
        let running = app.is_running(svc);
        let icon = if running { "●" } else { "○" };
        let is_sel = app.section == Section::Services && i == app.cursor;
        let (style, icon_style) = item_styles(is_sel, running);
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {icon} "), icon_style),
            Span::styled(svc.as_str(), style),
        ]))
    }).collect();

    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Services ").title_style(border).border_style(border)),
        split[0],
    );

    // Combos
    let items: Vec<ListItem> = app.combos.iter().enumerate().map(|(i, combo)| {
        let svcs = app.config.combinations.get(combo.as_str());
        let (total, running_n) = match svcs {
            Some(s) => (s.len(), s.iter().filter(|x| app.is_running(x)).count()),
            None => (0, 0),
        };
        let (icon, icon_color) = match (running_n, total) {
            (r, t) if r == t && t > 0 => ("●", Color::Green),
            (r, _) if r > 0 => ("◐", Color::Yellow),
            _ => ("○", Color::White),
        };
        let is_sel = app.section == Section::Combos && i == app.cursor;
        let style = if is_sel {
            if running_n == total && total > 0 {
                Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
            }
        } else if running_n > 0 {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let icon_style = if is_sel { style } else { Style::default().fg(icon_color) };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {icon} "), icon_style),
            Span::styled(format!("{combo:<12}"), style),
            Span::styled(format!(" {running_n}/{total}"), if is_sel { style } else { Style::default().fg(icon_color).add_modifier(Modifier::DIM) }),
        ]))
    }).collect();

    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Combinations ").title_style(border).border_style(border)),
        split[1],
    );
}

fn draw_log_panel(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Snapshot all needed state up front to avoid double-borrow issues
    let svc_name = app.selected_service_for_logs().map(|s| s.to_string());
    let running_combo = app.combo_running_services();
    let combo_count = running_combo.len();
    let combo_idx = app.combo_log_idx;
    let current_item = app.current_item().unwrap_or("").to_string();

    let mode_tag = if app.interactive_mode { " [INTERACTIVE] " } else { "" };
    let log_title = match &svc_name {
        Some(svc) => {
            if combo_count > 1 {
                let idx = combo_idx % combo_count;
                format!("{mode_tag}logs: {svc} [{}/{}] ", idx + 1, combo_count)
            } else {
                format!("{mode_tag}logs: {svc} ")
            }
        }
        None => {
            if !current_item.is_empty() { format!(" {current_item} (not running) ") } else { " no selection ".into() }
        }
    };

    let scroll_suffix = if app.log_scroll > 0 { format!("[+{}] ", app.log_scroll) } else { String::new() };
    let full_title = format!("{log_title}{scroll_suffix}");

    let border = if app.interactive_mode {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else if app.focus == Focus::Right {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let log_block = Block::default().borders(Borders::ALL).title(full_title).title_style(border).border_style(border);
    let inner_h = area.height.saturating_sub(2) as usize;
    let inner_w = area.width.saturating_sub(2);

    if svc_name.is_some() && inner_h > 0 && inner_w > 0 {
        app.sync_tmux_size(inner_w, inner_h as u16);
        app.ensure_log_cache(inner_h);

        let visible = app.get_visible_lines(inner_h).to_vec();
        f.render_widget(Paragraph::new(visible).block(log_block), area);

        // Show cursor in interactive mode
        if app.interactive_mode && app.log_scroll == 0 {
            if let Some(svc) = &svc_name {
                if let Some((cx, cy)) = crate::tmux::cursor_position(&app.session, svc) {
                    // cursor_y is relative to visible pane, map to our panel
                    // panel content starts at area.y + 1 (border), and we show last inner_h lines
                    let visible_lines = app.stripped_line_count.min(inner_h);
                    let panel_y_offset = inner_h.saturating_sub(visible_lines) as u16;
                    let abs_y = area.y + 1 + panel_y_offset + cy;
                    let abs_x = area.x + 1 + cx;
                    if abs_y < area.y + area.height - 1 && abs_x < area.x + area.width - 1 {
                        f.set_cursor_position((abs_x, abs_y));
                    }
                }
            }
        }
    } else {
        f.render_widget(
            Paragraph::new("select a running service to view logs")
                .block(log_block).style(Style::default().fg(Color::DarkGray))
                .alignment(ratatui::layout::Alignment::Center),
            area,
        );
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.search_mode {
        f.render_widget(Paragraph::new(Line::from(vec![
            Span::styled(" /", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}_", app.search_query), Style::default().bg(Color::Yellow).fg(Color::Black)),
        ])), area);
    } else {
        let text = {
            let msg = app.get_message();
            if !msg.is_empty() {
                msg.to_string()
            } else if !app.search_query.is_empty() {
                let total = app.search_matches.len();
                let cur = if total > 0 { app.search_current + 1 } else { 0 };
                format!("search: \"{}\" [{cur}/{total}] (n/N navigate, Esc clear)", app.search_query)
            } else {
                let r = app.services.iter().filter(|s| app.is_running(s)).count();
                format!("{r}/{} services running", app.services.len())
            }
        };
        f.render_widget(
            Paragraph::new(format!(" {text}")).style(Style::default().bg(Color::White).fg(Color::Black)),
            area,
        );
    }
}

fn draw_copy_mode(f: &mut Frame, app: &mut App) {
    let size = f.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .split(size);

    let svc_name = app.selected_service_for_logs().unwrap_or("").to_string();
    let title = if !svc_name.is_empty() {
        format!(" COPY MODE — {svc_name} ")
    } else {
        " COPY MODE ".into()
    };
    f.render_widget(
        Paragraph::new(title).style(Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
        layout[0],
    );

    let inner_h = layout[1].height as usize;
    let inner_w = layout[1].width;

    if !svc_name.is_empty() && inner_h > 0 {
        app.sync_tmux_size(inner_w, inner_h as u16);
        app.ensure_log_cache(inner_h);

        let visible = app.get_visible_lines(inner_h).to_vec();
        f.render_widget(Paragraph::new(visible), layout[1]);
    } else {
        f.render_widget(Paragraph::new("no running service selected"), layout[1]);
    }

    if app.search_mode {
        f.render_widget(Paragraph::new(Line::from(vec![
            Span::styled(" /", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}_", app.search_query), Style::default().bg(Color::Yellow).fg(Color::Black)),
        ])), layout[2]);
    } else {
        let hint = if !app.search_query.is_empty() {
            let total = app.search_matches.len();
            let cur = if total > 0 { app.search_current + 1 } else { 0 };
            format!(" / search | n/N navigate [{cur}/{total}] | j/k scroll | Esc exit ")
        } else {
            " / search | j/k scroll | G bottom | g top | Esc exit ".into()
        };
        f.render_widget(
            Paragraph::new(hint).style(Style::default().bg(Color::Yellow).fg(Color::Black)),
            layout[2],
        );
    }
}

fn item_styles(is_sel: bool, running: bool) -> (Style, Style) {
    let style = if is_sel {
        if running {
            Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        }
    } else if running {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White).add_modifier(Modifier::DIM)
    };
    let icon_style = if is_sel { style } else if running {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::White)
    };
    (style, icon_style)
}
