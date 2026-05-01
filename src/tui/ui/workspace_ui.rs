use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use super::super::app::App;

pub(super) fn draw_popups(f: &mut Frame, app: &App, size: Rect) {
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
}

fn draw_shared_info(f: &mut Frame, app: &App, area: Rect) {
    let session = &app.session;
    let project = format!("{session}-shared");

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
    let branch_max = max_w.saturating_sub(22);

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
