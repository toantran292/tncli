use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use super::super::app::App;

pub(super) fn draw_popups(f: &mut Frame, app: &App, size: Rect) {
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
    let search_row = 1u16;
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
