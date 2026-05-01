use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use super::app::{App, ComboItem};

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

    draw_left_panel(f, app, outer[0]);

    let hints: &[(&str, &str)] =
        &[("s","start"),("x","stop"),("r","restart"),("g","git"),("?","help")];

    draw_bottom_bar(f, app, outer[1], hints);

    // Shortcuts popup
    if app.shortcuts_open {
        draw_shortcuts_popup(f, app, size);
    }
    if app.branch_menu_open {
        draw_branch_menu(f, app, size);
    }
    if app.wt_menu_open {
        draw_wt_menu(f, app, size);
    }
    if app.wt_name_input_open {
        draw_name_input(f, app, size);
    }
    if app.confirm_open {
        draw_confirm_dialog(f, app, size);
    }
    if app.shared_info_open {
        draw_shared_info(f, app, size);
    }
    if app.cheatsheet_open {
        draw_cheatsheet(f, size);
    }
    if app.ws_select_open {
        draw_ws_select(f, app, size);
    }
    if app.ws_edit_open {
        draw_ws_edit_menu(f, app, size);
    }
    if app.ws_add_open {
        draw_ws_add_picker(f, app, size);
    }
    if app.ws_remove_open {
        draw_ws_remove_picker(f, app, size);
    }
    // Branch picker renders last — always on top of other popups
    if app.wt_branch_open {
        draw_branch_picker(f, app, size);
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

fn draw_name_input(f: &mut Frame, app: &App, area: Rect) {
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 5u16;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let title = format!(" New branch (from {}) ", app.wt_name_base_branch);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Magenta));

    let input_text = Line::from(vec![
        Span::styled(" > ", Style::default().fg(Color::Cyan)),
        Span::styled(format!("{}_", app.wt_name_input), Style::default()),
    ]);
    let hint = Line::from(Span::styled(
        " Enter to create, Esc to cancel",
        Style::default().fg(Color::DarkGray),
    ));

    let content = Paragraph::new(vec![input_text, hint]).block(block);
    f.render_widget(Clear, popup_area);
    f.render_widget(content, popup_area);
}

fn draw_confirm_dialog(f: &mut Frame, app: &App, area: Rect) {
    let msg = &app.confirm_msg;
    let width = (msg.len() as u16 + 4).min(area.width.saturating_sub(4));
    let height = 3;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" Confirm ")
        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));

    let text = Paragraph::new(Line::from(vec![
        Span::styled(msg.as_str(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]))
    .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(Clear, popup_area);
    f.render_widget(text.block(block), popup_area);
}

fn draw_branch_menu(f: &mut Frame, app: &App, area: Rect) {
    let options = ["checkout branch  /", "create new branch", "pull remote"];
    let width = 32u16.min(area.width.saturating_sub(4));
    let height = (options.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let items: Vec<ListItem> = options.iter().enumerate().map(|(i, opt)| {
        let is_sel = i == app.branch_menu_cursor;
        let style = if is_sel {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(format!(" {opt} "), style))
    }).collect();

    let alias = app.config.repos.get(&app.branch_menu_dir)
        .and_then(|d| d.alias.as_deref())
        .unwrap_or(&app.branch_menu_dir);
    let title = format!(" Branch: {alias} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Clear, popup_area);
    f.render_widget(List::new(items).block(block), popup_area);
}

fn draw_wt_menu(f: &mut Frame, app: &App, area: Rect) {
    let options = ["Create from current branch", "Pick branch...", "Refresh worktrees", "Bind main to 127.0.0.1", "Delete worktree"];
    let width = 40u16.min(area.width.saturating_sub(4));
    let height = (options.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let items: Vec<ListItem> = options.iter().enumerate().map(|(i, opt)| {
        let style = if i == app.wt_menu_cursor {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(format!(" {opt}"), style))
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Worktree: {} (Esc to close) ", app.wt_menu_dir))
        .title_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Magenta));

    f.render_widget(Clear, popup_area);
    f.render_widget(List::new(items).block(block), popup_area);
}

fn draw_branch_picker(f: &mut Frame, app: &App, area: Rect) {
    let width = 55u16.min(area.width.saturating_sub(4));
    let max_visible = 15usize;
    let search_row = 1u16; // extra row for search bar
    let filtered = &app.wt_branch_filtered;
    let height = (filtered.len().min(max_visible) as u16 + 2 + search_row).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let scroll = if app.wt_branch_cursor >= max_visible {
        app.wt_branch_cursor - max_visible + 1
    } else {
        0
    };

    let mut items: Vec<ListItem> = Vec::new();

    // Search bar row
    let search_display = if app.wt_branch_searching {
        format!(" /{}_", app.wt_branch_search)
    } else if !app.wt_branch_search.is_empty() {
        format!(" /{} (/ to edit)", app.wt_branch_search)
    } else {
        " / to search".to_string()
    };
    items.push(ListItem::new(Span::styled(
        search_display,
        if app.wt_branch_searching {
            Style::default().bg(Color::Yellow).fg(Color::Black)
        } else {
            Style::default().fg(Color::DarkGray)
        },
    )));

    // Branch items
    for (i, branch) in filtered.iter().enumerate().skip(scroll).take(max_visible) {
        let style = if i == app.wt_branch_cursor {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let max_len = width as usize - 4;
        let display = if branch.len() > max_len {
            format!(" {}...", &branch[..max_len.saturating_sub(3)])
        } else {
            format!(" {branch}")
        };
        items.push(ListItem::new(Span::styled(display, style)));
    }

    let title = format!(" Branches ({}/{}) ", filtered.len(), app.wt_branches.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Magenta));

    f.render_widget(Clear, popup_area);
    f.render_widget(List::new(items).block(block), popup_area);
}

/// Build a Line with left spans + right-aligned counter within given width.
fn right_align_line<'a>(left_spans: Vec<Span<'a>>, counter: &str, counter_style: Style, row_style: Style, is_sel: bool, width: usize) -> Line<'a> {
    // Use char count (display width) not byte length for unicode support
    let left_len: usize = left_spans.iter().map(|s| s.content.chars().count()).sum();
    let counter_len = counter.chars().count();
    let pad = width.saturating_sub(left_len + counter_len);
    let mut spans = left_spans;
    if pad > 0 {
        spans.push(Span::styled(" ".repeat(pad), if is_sel { row_style } else { Style::default() }));
    }
    spans.push(Span::styled(counter.to_string(), if is_sel { row_style } else { counter_style }));
    Line::from(spans)
}

fn draw_left_panel(f: &mut Frame, app: &App, area: Rect) {
    let inner_w = area.width as usize;
    let single_combo = app.combos.len() <= 1;
    let combo_list: Vec<ListItem> = app.combo_items.iter().enumerate().filter_map(|(i, item)| {
        let is_sel = i == app.cursor;
        let next = app.combo_items.get(i + 1);
        // Is this the last Instance under its Combo?
        let _is_last_instance = matches!(next, Some(ComboItem::Combo(_)) | None);
        // Is this the last InstanceDir under its Instance? (skip over child services)
        let is_last_dir = {
            let mut j = i + 1;
            // Skip over InstanceService children
            while j < app.combo_items.len() {
                if matches!(app.combo_items[j], ComboItem::InstanceService { .. }) { j += 1; } else { break; }
            }
            !matches!(app.combo_items.get(j), Some(ComboItem::InstanceDir { .. }))
        };
        // Is this the last InstanceService under its InstanceDir?
        let is_last_svc = !matches!(next, Some(ComboItem::InstanceService { .. }));
        let list_item = match item {
            ComboItem::Combo(combo_name) => {
                if single_combo {
                    return None;
                }
                {
                    let entries = app.config.all_workspaces().get(combo_name.as_str()).cloned().unwrap_or_default();
                    let total = entries.len();
                    let running_n = entries.iter().filter(|entry| {
                        app.config.find_service_entry_quiet(entry)
                            .map(|(dir, svc)| {
                                let alias = app.config.repos.get(&dir)
                                    .and_then(|d| d.alias.as_deref())
                                    .unwrap_or(dir.as_str());
                                app.is_running(&format!("{alias}~{svc}"))
                            })
                            .unwrap_or(false)
                    }).count();

                    let (icon, icon_color) = match (running_n, total) {
                        (r, t) if r == t && t > 0 => ("●", Color::Green),
                        (r, _) if r > 0 => ("◐", Color::Yellow),
                        _ => ("○", Color::White),
                    };

                    let style = if is_sel {
                        Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else if running_n > 0 {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().add_modifier(Modifier::DIM)
                    };
                    let icon_style = if is_sel { style } else { Style::default().fg(icon_color) };

                    let counter = format!("{running_n}/{total}");
                    let counter_style = Style::default().fg(icon_color).add_modifier(Modifier::DIM);
                    ListItem::new(right_align_line(
                        vec![
                            Span::styled(format!(" {icon} "), icon_style),
                            Span::styled(combo_name.as_str(), style),
                        ],
                        &counter, counter_style, style, is_sel, inner_w,
                    ))
                }
            }
            ComboItem::Instance { branch, is_main } => {
                let is_deleting = !is_main && app.deleting_workspaces.contains(branch);
                let is_creating = !is_main && app.creating_workspaces.contains(branch);

                if is_creating {
                    let style = if is_sel {
                        Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM)
                    };
                    let br_display = if branch.len() > 12 { format!("{}...", &branch[..10]) } else { branch.clone() };
                    let progress = app.active_pipelines.iter()
                        .find(|p| p.branch == *branch)
                        .map(|p| {
                            if p.total_stages > 0 {
                                let pct = (p.current_stage * 100) / p.total_stages;
                                format!("{pct}%")
                            } else {
                                "...".to_string()
                            }
                        })
                        .unwrap_or_else(|| "...".to_string());
                    let counter_style = if is_sel { style } else { Style::default().fg(Color::Yellow) };
                    ListItem::new(right_align_line(
                        vec![
                            Span::styled("~ ", style),
                            Span::styled(br_display, style),
                        ],
                        &progress, counter_style, style, is_sel, inner_w,
                    ))
                } else if is_deleting {
                    let style = if is_sel {
                        Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Red).add_modifier(Modifier::DIM)
                    };
                    let br_display = if branch.len() > 12 { format!("{}...", &branch[..10]) } else { branch.clone() };
                    let progress = app.active_pipelines.iter()
                        .find(|p| p.branch == *branch)
                        .map(|p| {
                            if p.total_stages > 0 {
                                let pct = (p.current_stage * 100) / p.total_stages;
                                format!("{pct}%")
                            } else {
                                "...".to_string()
                            }
                        })
                        .unwrap_or_else(|| "...".to_string());
                    let counter_style = if is_sel { style } else { Style::default().fg(Color::Red) };
                    ListItem::new(right_align_line(
                        vec![
                            Span::styled("~ ", style),
                            Span::styled(br_display, style),
                        ],
                        &progress, counter_style, style, is_sel, inner_w,
                    ))
                } else {
                    let (running, total) = if *is_main {
                        // For main: count using alias~svc tmux name format
                        let combo_name = app.combo_items.iter().rev()
                            .skip(app.combo_items.len() - i)
                            .find_map(|ci| if let ComboItem::Combo(name) = ci { Some(name.clone()) } else { None })
                            .unwrap_or_default();
                        let entries = app.config.all_workspaces().get(&combo_name).cloned().unwrap_or_default();
                        let total = entries.len();
                        let running = entries.iter().filter(|entry| {
                            app.config.find_service_entry_quiet(entry)
                                .map(|(dir, svc)| {
                                    let alias = app.config.repos.get(&dir)
                                        .and_then(|d| d.alias.as_deref())
                                        .unwrap_or(dir.as_str());
                                    let tmux_name = format!("{alias}~{svc}");
                                    app.is_running(&tmux_name)
                                })
                                .unwrap_or(false)
                        }).count();
                        (running, total)
                    } else {
                        app.worktrees.values()
                            .filter(|wt| super::app::workspace_branch_eq(wt, branch))
                            .fold((0, 0), |(r, t), wt| {
                                let alias = app.config.repos.get(&wt.parent_dir)
                                    .and_then(|d| d.alias.as_deref()).unwrap_or(&wt.parent_dir);
                                let branch_safe = branch.replace('/', "-");
                                let all_svcs = app.config.all_services_for(&wt.parent_dir);
                                let dir_svcs = all_svcs.len();
                                let dir_running = all_svcs.iter()
                                    .filter(|s| app.is_running(&format!("{alias}~{s}~{branch_safe}")))
                                    .count();
                                (r + dir_running, t + dir_svcs)
                            })
                    };

                    let counter = format!("{running}/{total}");
                    let counter_color = if running == total && total > 0 { Color::Green }
                        else if running > 0 { Color::Yellow }
                        else { Color::DarkGray };
                    let icon = if running == total && total > 0 { "●" }
                        else if running > 0 { "◐" }
                        else { "○" };

                    let style = if is_sel {
                        Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
                    };
                    let icon_style = if is_sel { style } else { Style::default().fg(counter_color) };

                    let is_expanded = matches!(next, Some(ComboItem::InstanceDir { .. }));
                    let collapse_icon = if is_expanded { "▾" } else { "▸" };
                    let br_display = if branch.len() > 12 { format!("{}...", &branch[..10]) } else { branch.clone() };

                    let counter_style = Style::default().fg(counter_color);
                    ListItem::new(right_align_line(
                        vec![
                            Span::styled(format!("{collapse_icon}"), if is_sel { style } else { Style::default().fg(Color::DarkGray) }),
                            Span::styled(format!("{icon} "), icon_style),
                            Span::styled(br_display, style),
                        ],
                        &counter, counter_style, style, is_sel, inner_w,
                    ))
                }
            }
            ComboItem::InstanceDir { branch, dir, wt_key, is_main } => {
                let dir_prefix = if is_last_dir {
                    " └"
                } else {
                    " ├"
                };

                // Worktree-level global service — render as simple service item
                if let Some(svc_name) = dir.strip_prefix("_global:") {
                    let tmux_name = if *is_main {
                        format!("_global~{svc_name}")
                    } else {
                        let bs = branch.replace('/', "-");
                        format!("_global~{svc_name}~{bs}")
                    };
                    let running = app.is_running(&tmux_name);
                    let icon = if running { "◆" } else { "◇" };
                    let style = if is_sel {
                        Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else if running {
                        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    let icon_style = if is_sel { style } else if running {
                        Style::default().fg(Color::Magenta)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    let _ = (wt_key, is_main);
                    return Some(ListItem::new(Line::from(vec![
                        Span::styled(format!("{dir_prefix} "), if is_sel { style } else { Style::default().fg(Color::DarkGray) }),
                        Span::styled(format!("{icon} "), icon_style),
                        Span::styled(svc_name, style),
                    ])));
                }

                let alias = app.config.repos.get(dir).and_then(|d| d.alias.as_deref()).unwrap_or(dir.as_str());
                let (running, total) = if *is_main {
                    let all_svcs = app.config.all_services_for(dir);
                    let t = all_svcs.len();
                    let r = all_svcs.iter()
                        .filter(|s| app.is_running(&format!("{alias}~{s}")))
                        .count();
                    (r, t)
                } else {
                    let branch_safe = branch.replace('/', "-");
                    let all_svcs = app.config.all_services_for(dir);
                    let t = all_svcs.len();
                    let r = all_svcs.iter()
                        .filter(|s| app.is_running(&format!("{alias}~{s}~{branch_safe}")))
                        .count();
                    (r, t)
                };

                let counter = format!("{running}/{total}");
                let counter_color = if running == total && total > 0 { Color::Green }
                    else if running > 0 { Color::Yellow }
                    else { Color::DarkGray };

                let display_name = alias.to_string();
                let icon = if running == total && total > 0 { "●" }
                    else if running > 0 { "◐" }
                    else { "○" };
                let icon_color = counter_color;

                let style = if is_sel {
                    Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                };

                let counter_style = Style::default().fg(counter_color);
                let left_spans = vec![
                    Span::styled(format!("{dir_prefix} "), if is_sel { style } else { Style::default().fg(Color::DarkGray) }),
                    Span::styled(format!("{icon} "), if is_sel { style } else { Style::default().fg(icon_color) }),
                    Span::styled(display_name, style),
                ];
                let spans = right_align_line(left_spans, &counter, counter_style, style, is_sel, inner_w).spans;

                let _ = (wt_key, is_main); // used by app logic, not rendering
                ListItem::new(Line::from(spans))
            }
            ComboItem::InstanceService { svc, tmux_name, .. } => {
                let running = app.is_running(tmux_name);
                let stopping = app.is_stopping(tmux_name);
                let starting = app.is_starting(tmux_name);
                let is_global = app.config.is_global_service(svc);
                let icon = if stopping { "~" } else if starting { "~" } else if running {
                    if is_global { "◆" } else { "●" }
                } else {
                    if is_global { "◇" } else { "○" }
                };

                let style = if is_sel {
                    if stopping || starting {
                        Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else if running {
                        Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                    }
                } else if stopping || starting {
                    Style::default().fg(Color::Yellow)
                } else if running {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White).add_modifier(Modifier::DIM)
                };

                let icon_style = if is_sel { style } else if stopping || starting {
                    Style::default().fg(Color::Yellow)
                } else if running {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };

                let svc_char = if is_last_svc { "└" } else { "├" };
                // Check if parent dir is last dir (to show │ or space)
                let parent_is_last = {
                    // Find next item after all services of this dir
                    let mut j = i + 1;
                    while j < app.combo_items.len() {
                        match &app.combo_items[j] {
                            ComboItem::InstanceService { .. } => j += 1,
                            ComboItem::InstanceDir { .. } => break,
                            _ => break,
                        }
                    }
                    !matches!(app.combo_items.get(j), Some(ComboItem::InstanceDir { .. }))
                };
                let tree_prefix = if parent_is_last { "  " } else { " │" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{tree_prefix} {svc_char} "), if is_sel { icon_style } else { Style::default().fg(Color::DarkGray) }),
                    Span::styled(format!("{icon} "), icon_style),
                    Span::styled(svc.as_str(), style),
                ]))
            }
        };
        Some(list_item)
    }).collect();

    let mut combo_state = ratatui::widgets::ListState::default();
    // Adjust cursor for filtered-out Combo items
    let visual_cursor = if single_combo {
        let skipped = app.combo_items.iter().take(app.cursor + 1)
            .filter(|ci| matches!(ci, ComboItem::Combo(_)))
            .count();
        app.cursor.saturating_sub(skipped)
    } else {
        app.cursor
    };
    combo_state.select(Some(visual_cursor));
    let block = Block::default();
    f.render_stateful_widget(
        List::new(combo_list).block(block),
        area,
        &mut combo_state,
    );
}

fn draw_bottom_bar(f: &mut Frame, app: &App, area: Rect, hints: &[(&str, &str)]) {
    let msg = app.get_message();
        if !msg.is_empty() {
            // Message takes over the bar
            f.render_widget(
                Paragraph::new(format!(" {msg}"))
                    .style(Style::default().bg(Color::White).fg(Color::Black)),
                area,
            );
        } else {
            // Key hints — truncate to fit terminal width
            let max_w = area.width as usize;
            let mut hint_spans: Vec<Span> = Vec::new();
            let mut used = 0;
            for (key, desc) in hints.iter() {
                let key_part = format!(" {key} ");
                let desc_part = format!(" {desc} ");
                let entry_w = key_part.len() + desc_part.len();
                if used + entry_w > max_w {
                    // Try key-only for remaining hints
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
}

fn draw_shared_info(f: &mut Frame, app: &App, area: Rect) {
    let session = &app.session;
    let project = format!("{session}-shared");

    // Query docker for container status (cached per frame via popup open)
    let container_status = get_shared_container_status(&project);

    let mut lines: Vec<Line> = Vec::new();
    for (name, svc) in &app.config.shared_services {
        let alt_key = format!("{project}-{name}-1");
        let is_running = container_status.get(name.as_str())
            .or_else(|| container_status.get(alt_key.as_str()))
            .copied()
            .unwrap_or(false);

        let icon = if is_running { "●" } else { "○" };
        let icon_color = if is_running { Color::Green } else { Color::DarkGray };
        let host = svc.host.as_deref().unwrap_or("-");
        let ports: String = svc.ports.iter()
            .map(|p| {
                let parts: Vec<&str> = p.split(':').collect();
                if parts.len() == 2 { parts[0].to_string() } else { p.clone() }
            })
            .collect::<Vec<_>>()
            .join(", ");

        let mut spans = vec![
            Span::styled(format!(" {icon} "), Style::default().fg(icon_color)),
            Span::styled(
                format!("{name:<16}"),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{host:<22}"), Style::default().fg(Color::Cyan)),
            Span::styled(format!(":{ports}"), Style::default().fg(Color::Yellow)),
        ];
        if let Some(cap) = svc.capacity {
            spans.push(Span::styled(
                format!("  (cap:{cap})"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(spans));
    }

    // Add legend
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" ● ", Style::default().fg(Color::Green)),
        Span::raw("running  "),
        Span::styled("○ ", Style::default().fg(Color::DarkGray)),
        Span::raw("stopped   "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw(" close"),
    ]));

    let width = 62u16.min(area.width.saturating_sub(4));
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Shared Services ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Cyan));

    let text = Paragraph::new(lines).block(block);
    f.render_widget(Clear, popup_area);
    f.render_widget(text, popup_area);
}

fn draw_cheatsheet(f: &mut Frame, area: Rect) {
    let key = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let desc = Style::default().fg(Color::White);
    let header = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    let lines: Vec<Line> = vec![
        Line::from(Span::styled(" Left Panel", header)),
        Line::from(vec![Span::styled("  j/k        ", key), Span::styled("Navigate up/down", desc)]),
        Line::from(vec![Span::styled("  Enter/Space ", key), Span::styled("Toggle start/stop or collapse", desc)]),
        Line::from(vec![Span::styled("  s           ", key), Span::styled("Start service/instance", desc)]),
        Line::from(vec![Span::styled("  x           ", key), Span::styled("Stop service/instance", desc)]),
        Line::from(vec![Span::styled("  X           ", key), Span::styled("Stop all (confirm)", desc)]),
        Line::from(vec![Span::styled("  r           ", key), Span::styled("Restart", desc)]),
        Line::from(vec![Span::styled("  c           ", key), Span::styled("Shortcuts popup", desc)]),
        Line::from(vec![Span::styled("  e           ", key), Span::styled("Open in editor", desc)]),
        Line::from(vec![Span::styled("  b           ", key), Span::styled("Branch: pull (main) / menu (wt)", desc)]),
        Line::from(vec![Span::styled("  w           ", key), Span::styled("Create workspace / worktree menu", desc)]),
        Line::from(vec![Span::styled("  d           ", key), Span::styled("Delete workspace (confirm)", desc)]),
        Line::from(vec![Span::styled("  t           ", key), Span::styled("Open shell in directory", desc)]),
        Line::from(vec![Span::styled("  I           ", key), Span::styled("Shared services info", desc)]),
        Line::from(vec![Span::styled("  R           ", key), Span::styled("Reload config", desc)]),
        Line::from(vec![Span::styled("  Tab/l       ", key), Span::styled("Focus log panel", desc)]),
        Line::from(""),
        Line::from(Span::styled(" Right Panel (Logs)", header)),
        Line::from(vec![Span::styled("  j/k         ", key), Span::styled("Scroll down/up", desc)]),
        Line::from(vec![Span::styled("  G/g         ", key), Span::styled("Jump to bottom/top", desc)]),
        Line::from(vec![Span::styled("  /           ", key), Span::styled("Search in logs", desc)]),
        Line::from(vec![Span::styled("  n/N         ", key), Span::styled("Next/prev match or cycle service", desc)]),
        Line::from(vec![Span::styled("  i           ", key), Span::styled("Interactive mode (send keys)", desc)]),
        Line::from(vec![Span::styled("  y           ", key), Span::styled("Copy mode (fullscreen)", desc)]),
        Line::from(vec![Span::styled("  Tab/h       ", key), Span::styled("Focus left panel", desc)]),
        Line::from(""),
        Line::from(Span::styled(" Global", header)),
        Line::from(vec![Span::styled("  a           ", key), Span::styled("Attach to tmux session", desc)]),
        Line::from(vec![Span::styled("  ?           ", key), Span::styled("This cheat-sheet", desc)]),
        Line::from(vec![Span::styled("  q           ", key), Span::styled("Quit", desc)]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Esc", Style::default().fg(Color::Yellow)),
            Span::raw(" close"),
        ]),
    ];

    let width = 50u16.min(area.width.saturating_sub(4));
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Keybindings ")
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Clear, popup_area);
    f.render_widget(Paragraph::new(lines).block(block), popup_area);
}

fn draw_ws_select(f: &mut Frame, app: &App, area: Rect) {
    let max_w = area.width.saturating_sub(6) as usize;
    let branch_max = max_w.saturating_sub(22); // space for " [x] alias    branch: "

    let items: Vec<ListItem> = app.ws_select_items.iter().enumerate().map(|(i, item)| {
        let is_sel = i == app.ws_select_cursor;
        let check = if item.selected { "[x]" } else { "[ ]" };
        let warn = if item.conflict { " !" } else { "" };
        let branch_display = if item.selected {
            if item.branch.len() > branch_max {
                format!("{}...", &item.branch[..branch_max.saturating_sub(3)])
            } else {
                item.branch.clone()
            }
        } else { "-".to_string() };
        let text = format!(" {check} {:<10} {branch_display}{warn}", item.alias);
        let style = if is_sel {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else if item.conflict {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(text, style))
    }).collect();

    let footer = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" Space", Style::default().fg(Color::Yellow)),
            Span::raw("=toggle "),
            Span::styled("b", Style::default().fg(Color::Yellow)),
            Span::raw("=branch "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw("=create "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw("=cancel"),
        ]),
    ];

    let width = 40u16.max(area.width / 3).min(area.width.saturating_sub(4));
    let height = (items.len() as u16 + footer.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let title = format!(" Create workspace: {} ", app.ws_select_branch);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Green));

    // Split popup into list + footer
    let inner = block.inner(popup_area);
    let list_height = inner.height.saturating_sub(footer.len() as u16);

    let footer_height = footer.len() as u16;
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);
    f.render_widget(
        List::new(items),
        Rect::new(inner.x, inner.y, inner.width, list_height),
    );
    f.render_widget(
        Paragraph::new(footer),
        Rect::new(inner.x, inner.y + list_height, inner.width, footer_height),
    );
}

fn draw_ws_edit_menu(f: &mut Frame, app: &App, area: Rect) {
    let options = ["Add repo", "Remove repo"];
    let items: Vec<ListItem> = options.iter().enumerate().map(|(i, opt)| {
        let is_sel = i == app.ws_edit_cursor;
        let style = if is_sel {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(format!(" {opt} "), style))
    }).collect();

    let width = 28u16.min(area.width.saturating_sub(4));
    let height = (options.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let title = format!(" Workspace: {} ", app.ws_edit_branch);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Clear, popup_area);
    f.render_widget(List::new(items).block(block), popup_area);
}

fn draw_ws_add_picker(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app.ws_add_items.iter().enumerate().map(|(i, item)| {
        let is_sel = i == app.ws_add_cursor;
        let warn = if item.conflict { " \u{26a0}" } else { "" };
        let text = format!(" {:<12} branch: {}{warn}", item.alias, item.branch);
        let style = if is_sel {
            Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else if item.conflict {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(text, style))
    }).collect();

    let width = 48u16.min(area.width.saturating_sub(4));
    let height = (items.len() as u16 + 4).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let title = format!(" Add repo to {} ", app.ws_edit_branch);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(popup_area);
    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);
    f.render_widget(
        List::new(items),
        Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1)),
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" b", Style::default().fg(Color::Yellow)),
            Span::raw("=branch  "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw("=add  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw("=cancel"),
        ])),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

fn draw_ws_remove_picker(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app.ws_remove_items.iter().enumerate().map(|(i, (dir_name, _))| {
        let is_sel = i == app.ws_remove_cursor;
        let alias = app.config.repos.get(dir_name)
            .and_then(|d| d.alias.as_deref())
            .unwrap_or(dir_name);
        let style = if is_sel {
            Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(format!(" {alias} ({dir_name}) "), style))
    }).collect();

    let width = 40u16.min(area.width.saturating_sub(4));
    let height = (items.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    let title = format!(" Remove from {} ", app.ws_edit_branch);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(Color::Red));

    f.render_widget(Clear, popup_area);
    f.render_widget(List::new(items).block(block), popup_area);
}

/// Query docker for running containers in the shared project.
/// Returns map of service_name → is_running.
fn get_shared_container_status(project: &str) -> std::collections::HashMap<String, bool> {
    let mut status = std::collections::HashMap::new();
    let output = std::process::Command::new("docker")
        .args(["compose", "-p", project, "ps", "--format", "{{.Service}} {{.State}}"])
        .output();
    if let Ok(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() == 2 {
                status.insert(parts[0].to_string(), parts[1] == "running");
            }
        }
    }
    status
}
