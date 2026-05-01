use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem};
use ratatui::Frame;

use super::super::app::{App, ComboItem};

/// Build a Line with left spans + right-aligned counter within given width.
pub(super) fn right_align_line<'a>(left_spans: Vec<Span<'a>>, counter: &str, counter_style: Style, row_style: Style, is_sel: bool, width: usize) -> Line<'a> {
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

pub(super) fn draw_left_panel(f: &mut Frame, app: &App, area: Rect) {
    let inner_w = area.width as usize;
    let br_max = inner_w.saturating_sub(8); // prefix (~4) + counter (~4)
    let single_combo = app.combos.len() <= 1;
    let combo_list: Vec<ListItem> = app.combo_items.iter().enumerate().filter_map(|(i, item)| {
        let is_sel = i == app.cursor;
        let next = app.combo_items.get(i + 1);
        let _is_last_instance = matches!(next, Some(ComboItem::Combo(_)) | None);
        let is_last_dir = {
            let mut j = i + 1;
            while j < app.combo_items.len() {
                if matches!(app.combo_items[j], ComboItem::InstanceService { .. }) { j += 1; } else { break; }
            }
            !matches!(app.combo_items.get(j), Some(ComboItem::InstanceDir { .. }))
        };
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
                    let br_display = if branch.len() > br_max { format!("{}...", &branch[..br_max.saturating_sub(3)]) } else { branch.clone() };
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
                    let br_display = if branch.len() > br_max { format!("{}...", &branch[..br_max.saturating_sub(3)]) } else { branch.clone() };
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
                            .filter(|wt| super::super::app::workspace_branch_eq(wt, branch))
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
                    let br_display = if branch.len() > br_max { format!("{}...", &branch[..br_max.saturating_sub(3)]) } else { branch.clone() };

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

                if let Some(svc_name) = dir.strip_prefix("_global:") {
                    let tmux_name = if *is_main {
                        format!("_global~{svc_name}")
                    } else {
                        let bs = crate::services::branch_safe(branch);
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
                    let icon_style = if is_sel {
                        Style::default().bg(Color::Cyan).fg(Color::Black)
                    } else if running {
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

                let _ = (wt_key, is_main);
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
                let parent_is_last = {
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
    let visual_cursor = if single_combo {
        let skipped = app.combo_items.iter().take(app.cursor + 1)
            .filter(|ci| matches!(ci, ComboItem::Combo(_)))
            .count();
        app.cursor.saturating_sub(skipped)
    } else {
        app.cursor
    };
    combo_state.select(Some(visual_cursor));
    f.render_stateful_widget(List::new(combo_list).block(Block::default()), area, &mut combo_state);
}
