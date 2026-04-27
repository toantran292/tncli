use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use super::app::{App, Focus, Section, TreeItem};

const LEFT_W: u16 = 30;

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
            Constraint::Length(1),  // header
            Constraint::Length(1),  // gap
            Constraint::Min(5),    // panels
            Constraint::Length(1), // bottom bar
        ])
        .split(size);

    // Header: session name (left) + running count (right)
    let total: usize = app.config.dirs.values().map(|d| d.services.len()).sum();
    let running = app.config.all_services().iter()
        .filter(|(_, svc)| app.is_running(svc))
        .count();
    let stopping = app.stopping_services.len();
    let left = format!(" tncli -- {} ", app.session);
    let right = if stopping > 0 {
        format!(" {running}/{total} running, {stopping} stopping ")
    } else {
        format!(" {running}/{total} running ")
    };
    let pad_len = (size.width as usize).saturating_sub(left.len() + right.len());
    let right_color = if stopping > 0 { Color::Red } else if running > 0 { Color::Green } else { Color::DarkGray };
    f.render_widget(Paragraph::new(Line::from(vec![
        Span::styled(&left, Style::default().bg(Color::White).fg(Color::Black).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(pad_len), Style::default().bg(Color::White)),
        Span::styled(&right, Style::default().bg(Color::White).fg(right_color)),
    ])), outer[0]);

    // Panels
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LEFT_W), Constraint::Min(10)])
        .split(outer[2]);

    draw_left_panel(f, app, panels[0]);
    draw_log_panel(f, app, panels[1]);

    // Key hints
    let hints = if app.interactive_mode {
        &[("Esc","exit interactive"),("type","send to pane")][..]
    } else if app.focus == Focus::Left {
        &[("enter","toggle"),("s","start"),("x","stop"),("r","restart"),("c","cmds"),("t","shell"),("l/tab","logs"),("q","quit")][..]
    } else {
        &[("j/k","scroll"),("G","bottom"),("g","top"),("/","search"),("n/N","cycle"),("i","interact"),("h/tab","back"),("y","copy"),("q","quit")][..]
    };

    // Bottom bar: message/search if active, otherwise key hints
    draw_bottom_bar(f, app, outer[3], hints);

    // Shortcuts popup
    if app.shortcuts_open {
        draw_shortcuts_popup(f, app, size);
    }
}

fn draw_shortcuts_popup(f: &mut Frame, app: &App, area: Rect) {
    let width = 55u16.min(area.width.saturating_sub(4));
    let height = (app.shortcuts_items.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let items: Vec<ListItem> = app.shortcuts_items.iter().enumerate().map(|(i, sc)| {
        let is_sel = i == app.shortcuts_cursor;
        let style = if is_sel {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", sc.desc), style),
            Span::styled(
                format!("-> {}", sc.cmd),
                if is_sel { style } else { Style::default().fg(Color::DarkGray) },
            ),
        ]))
    }).collect();

    let title = format!(" Shortcuts: {} (Esc to close) ", app.shortcuts_title);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Clear, popup_area);
    f.render_widget(List::new(items).block(block), popup_area);
}

fn draw_left_panel(f: &mut Frame, app: &App, area: Rect) {
    let tree_h = app.tree_items.len() as u16 + 2;
    let combo_h = app.combos.len() as u16 + 2;

    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(tree_h), Constraint::Length(combo_h), Constraint::Min(0)])
        .split(area);

    let border = if app.focus == Focus::Left && app.section == Section::Services {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Services tree
    let items: Vec<ListItem> = app.tree_items.iter().enumerate().map(|(i, item)| {
        let is_sel = app.section == Section::Services && i == app.cursor;
        match item {
            TreeItem::Dir(dir_name) => {
                let dir = app.config.dirs.get(dir_name);
                let collapsed = app.dir_names.iter().position(|d| d == dir_name)
                    .and_then(|idx| app.dir_collapsed.get(idx).copied())
                    .unwrap_or(false);
                let arrow = if collapsed { ">" } else { "v" };
                let has_shortcuts = dir.is_some_and(|d| !d.shortcuts.is_empty());

                // Count running services in this dir
                let (running, total) = dir.map(|d| {
                    let total = d.services.len();
                    let running = d.services.keys().filter(|s| app.is_running(s)).count();
                    (running, total)
                }).unwrap_or((0, 0));

                let dir_style = if is_sel {
                    Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                };

                // Counter always visible
                let counter = format!("{running}/{total}");
                let counter_color = if running == total && total > 0 { Color::Green }
                    else if running > 0 { Color::Yellow }
                    else { Color::DarkGray };

                // Truncate dir name to fit: " v " (3) + name + " " + [c](3) + " " + counter + " "
                let max_name = (LEFT_W as usize)
                    .saturating_sub(3) // arrow
                    .saturating_sub(if has_shortcuts { 4 } else { 0 }) // [c]
                    .saturating_sub(counter.len() + 2); // counter + padding
                let display_name = if dir_name.len() > max_name && max_name > 3 {
                    format!("{}...", &dir_name[..max_name - 3])
                } else {
                    dir_name.to_string()
                };

                let mut spans = vec![
                    Span::styled(format!(" {arrow} "), if is_sel { dir_style } else { Style::default().fg(Color::Cyan) }),
                    Span::styled(display_name, dir_style),
                ];
                if has_shortcuts {
                    spans.push(Span::styled(" [c]", if is_sel { dir_style } else { Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM) }));
                }
                spans.push(Span::styled(format!(" {counter}"), if is_sel { dir_style } else { Style::default().fg(counter_color) }));

                ListItem::new(Line::from(spans))
            }
            TreeItem::Service { svc, .. } => {
                let running = app.is_running(svc);
                let stopping = app.is_stopping(svc);
                let icon = if stopping { "~" } else if running { "●" } else { "○" };
                let has_shortcuts = app.config.dirs.values()
                    .flat_map(|d| d.services.get(svc))
                    .any(|s| !s.shortcuts.is_empty());

                let style = if is_sel {
                    if stopping {
                        Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else if running {
                        Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                    }
                } else if stopping {
                    Style::default().fg(Color::Yellow)
                } else if running {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White).add_modifier(Modifier::DIM)
                };

                let icon_style = if is_sel { style } else if stopping {
                    Style::default().fg(Color::Yellow)
                } else if running {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };

                let mut spans = vec![
                    Span::styled(format!("   {icon} "), icon_style),
                    Span::styled(svc.as_str(), style),
                ];
                if has_shortcuts {
                    spans.push(Span::styled(" [c]", if is_sel { style } else { Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM) }));
                }
                ListItem::new(Line::from(spans))
            }
        }
    }).collect();

    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Services ").title_style(border).border_style(border)),
        split[0],
    );

    // Combinations
    let combo_border = if app.focus == Focus::Left && app.section == Section::Combos {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let combo_items: Vec<ListItem> = app.combos.iter().enumerate().map(|(i, combo)| {
        let entries = app.config.combinations.get(combo.as_str()).cloned().unwrap_or_default();
        let total = entries.len();
        let running_n = entries.iter().filter(|entry| {
            app.config.find_service_entry_quiet(entry)
                .map(|(_, svc)| app.is_running(&svc))
                .unwrap_or(false)
        }).count();

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
            Span::styled(format!("{combo:<14}"), style),
            Span::styled(format!(" {running_n}/{total}"), if is_sel { style } else { Style::default().fg(icon_color).add_modifier(Modifier::DIM) }),
        ]))
    }).collect();

    f.render_widget(
        List::new(combo_items).block(Block::default().borders(Borders::ALL).title(" Combinations ").title_style(combo_border).border_style(combo_border)),
        split[1],
    );
}

fn draw_log_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let svc_name = app.log_service_name();
    let cycle_info = app.log_cycle_info();

    let dir_name = app.selected_dir_name();
    let branch = dir_name.as_deref().and_then(|d| app.dir_branch(d));
    let branch_tag = branch.map(|b| format!("({b}) ")).unwrap_or_default();

    let log_title = match &svc_name {
        Some(svc) => {
            let mode_tag = if app.interactive_mode { "[INTERACTIVE] " } else { "" };
            if let Some((cur, total)) = cycle_info {
                format!(" {mode_tag}{branch_tag}logs: {svc} [{cur}/{total}] ")
            } else {
                format!(" {mode_tag}{branch_tag}logs: {svc} ")
            }
        }
        None => {
            match app.current_tree_item() {
                Some(TreeItem::Dir(d)) => {
                    let branch = app.dir_branch(d).map(|b| format!(" ({b})")).unwrap_or_default();
                    format!(" {d}{branch} ")
                }
                Some(TreeItem::Service { dir, svc, .. }) => {
                    let branch = app.dir_branch(dir).map(|b| format!(" ({b})")).unwrap_or_default();
                    format!(" {svc}{branch} (not running) ")
                }
                None => " no selection ".to_string(),
            }
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

        // Cursor in interactive mode
        if app.interactive_mode && app.log_scroll == 0 {
            if let Some(svc) = &app.log_service_name() {
                if let Some((cx, cy)) = crate::tmux::cursor_position(&app.session, svc) {
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

fn draw_bottom_bar(f: &mut Frame, app: &App, area: Rect, hints: &[(&str, &str)]) {
    if app.search_mode {
        // Search input
        f.render_widget(Paragraph::new(Line::from(vec![
            Span::styled(" /", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}_", app.search_query), Style::default().bg(Color::Yellow).fg(Color::Black)),
        ])), area);
    } else if !app.search_query.is_empty() {
        // Search results info
        let total = app.search_matches.len();
        let cur = if total > 0 { app.search_current + 1 } else { 0 };
        f.render_widget(
            Paragraph::new(format!(" search: \"{}\" [{cur}/{total}] (n/N navigate, Esc clear)", app.search_query))
                .style(Style::default().bg(Color::Yellow).fg(Color::Black)),
            area,
        );
    } else {
        let msg = app.get_message();
        if !msg.is_empty() {
            // Message takes over the bar
            f.render_widget(
                Paragraph::new(format!(" {msg}"))
                    .style(Style::default().bg(Color::White).fg(Color::Black)),
                area,
            );
        } else {
            // Key hints
            let hint_spans: Vec<Span> = hints.iter().flat_map(|(key, desc)| vec![
                Span::styled(format!(" {key} "), Style::default().bg(Color::White).fg(Color::Black)),
                Span::styled(format!(" {desc} "), Style::default().fg(Color::Yellow)),
            ]).collect();
            f.render_widget(Paragraph::new(Line::from(hint_spans)), area);
        }
    }
}

fn draw_copy_mode(f: &mut Frame, app: &mut App) {
    let size = f.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .split(size);

    let svc_name = app.log_service_name().unwrap_or_default();
    let title = if !svc_name.is_empty() {
        format!(" COPY MODE -- {svc_name} ")
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
