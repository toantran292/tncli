use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};

const RESULT_FILE: &str = "/tmp/tncli-popup-result";

// ── Text input ──

pub fn run_input() -> anyhow::Result<()> {
    let _ = std::fs::remove_file(RESULT_FILE);
    let mut terminal = ratatui::init();
    let result = input_loop(&mut terminal);
    ratatui::restore();
    result
}

fn input_loop(terminal: &mut ratatui::DefaultTerminal) -> anyhow::Result<()> {
    let mut input = String::new();

    loop {
        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(Clear, area);

            let input_line = Line::from(vec![
                Span::styled(" > ", Style::default().fg(Color::Cyan)),
                Span::raw(&input),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ]);
            let hint = Line::from(Span::styled(
                " Enter=confirm  Esc=cancel",
                Style::default().fg(Color::DarkGray),
            ));
            f.render_widget(Paragraph::new(vec![Line::from(""), input_line, hint]), area);
        })?;

        if !event::poll(Duration::from_millis(100))? { continue; }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter => {
                    if !input.trim().is_empty() {
                        std::fs::write(RESULT_FILE, input.trim())?;
                    }
                    return Ok(());
                }
                KeyCode::Esc => return Ok(()),
                KeyCode::Backspace => { input.pop(); }
                KeyCode::Char(c) => input.push(c),
                _ => {}
            }
        }
    }
}

// ── Workspace repo selection ──

struct WsItem {
    alias: String,
    source: String,
    target: String,
    path: String,
    selected: bool,
}

pub fn run_ws_select(items_raw: &str) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(RESULT_FILE);

    // format: alias|source|target|path[|selected] per item, comma-separated
    let mut items: Vec<WsItem> = items_raw
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.splitn(5, '|').collect();
            if parts.len() >= 4 {
                let selected = parts.get(4).map(|s| *s == "1").unwrap_or(true);
                Some(WsItem {
                    alias: parts[0].to_string(),
                    source: parts[1].to_string(),
                    target: parts[2].to_string(),
                    path: parts[3].to_string(),
                    selected,
                })
            } else {
                None
            }
        })
        .collect();

    if items.is_empty() { return Ok(()); }

    let mut terminal = ratatui::init();
    let result = ws_select_loop(&mut terminal, &mut items);
    ratatui::restore();
    result
}

fn serialize_items(items: &[WsItem]) -> String {
    items.iter().map(|i| {
        let sel = if i.selected { "1" } else { "0" };
        format!("{}|{}|{}|{}|{}", i.alias, i.source, i.target, i.path, sel)
    }).collect::<Vec<_>>().join(",")
}

fn ws_select_loop(
    terminal: &mut ratatui::DefaultTerminal,
    items: &mut Vec<WsItem>,
) -> anyhow::Result<()> {
    let mut cursor: usize = 0;

    loop {
        terminal.draw(|f| draw_ws_select(f, items, cursor))?;

        if !event::poll(Duration::from_millis(100))? { continue; }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
                KeyCode::Up | KeyCode::Char('k') => {
                    if cursor > 0 { cursor -= 1; }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if cursor + 1 < items.len() { cursor += 1; }
                }
                KeyCode::Char(' ') => items[cursor].selected = !items[cursor].selected,
                KeyCode::Char('b') => {
                    if items[cursor].selected && !items[cursor].path.is_empty() {
                        let items_data = serialize_items(items);
                        let result = format!("BRANCH_PICK:{}:{}", cursor, items_data);
                        std::fs::write(RESULT_FILE, result)?;
                        return Ok(());
                    }
                }
                KeyCode::Enter => {
                    let result: Vec<String> = items.iter()
                        .filter(|i| i.selected)
                        .map(|i| format!("{}:{}", i.alias, i.target))
                        .collect();
                    if !result.is_empty() {
                        std::fs::write(RESULT_FILE, result.join("\n"))?;
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

fn draw_ws_select(
    f: &mut ratatui::Frame,
    items: &[WsItem],
    cursor: usize,
) {
    let area = f.area();
    f.render_widget(Clear, area);

    let alias_w = items.iter().map(|i| i.alias.len()).max().unwrap_or(4).max(4);

    let list_items: Vec<ListItem> = items.iter().enumerate().map(|(i, item)| {
        let is_cur = i == cursor;
        let check = if item.selected { "[x]" } else { "[ ]" };

        if !item.selected {
            let text = format!(" {check} {:<alias_w$}  -", item.alias);
            let style = if is_cur {
                Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(Span::styled(text, style))
        } else {
            let prefix = format!(" {check} {:<alias_w$}  ", item.alias);
            let style = if is_cur {
                Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            if item.source != item.target {
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(
                        item.source.clone(),
                        if is_cur { style } else { Style::default().fg(Color::DarkGray) },
                    ),
                    Span::styled(" -> ", if is_cur { style } else { Style::default().fg(Color::DarkGray) }),
                    Span::styled(
                        item.target.clone(),
                        if is_cur { style } else { Style::default().fg(Color::Green) },
                    ),
                ]))
            } else {
                let text = format!("{prefix}{} -> {}", item.source, item.target);
                ListItem::new(Span::styled(text, style))
            }
        }
    }).collect();

    let footer = Line::from(vec![
        Span::styled(" Space", Style::default().fg(Color::Yellow)),
        Span::raw("=toggle "),
        Span::styled("b", Style::default().fg(Color::Yellow)),
        Span::raw("=branch "),
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::raw("=create "),
        Span::styled("Esc", Style::default().fg(Color::Yellow)),
        Span::raw("=cancel"),
    ]);

    let list_h = area.height.saturating_sub(2);
    f.render_widget(
        List::new(list_items),
        Rect::new(area.x, area.y, area.width, list_h),
    );
    f.render_widget(
        Paragraph::new(vec![Line::from(""), footer]),
        Rect::new(area.x, area.y + list_h, area.width, 2),
    );
}

// ── Confirm dialog ──

pub fn run_confirm() -> anyhow::Result<()> {
    let _ = std::fs::remove_file(RESULT_FILE);
    let mut terminal = ratatui::init();
    let result = confirm_loop(&mut terminal);
    ratatui::restore();
    result
}

fn confirm_loop(terminal: &mut ratatui::DefaultTerminal) -> anyhow::Result<()> {
    let mut selected = false; // false = No, true = Yes

    loop {
        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(Clear, area);

            let yes_style = if selected {
                Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let no_style = if !selected {
                Style::default().bg(Color::Red).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let btn_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(" Yes ", yes_style),
                Span::raw("   "),
                Span::styled(" No ", no_style),
            ]);
            let hint = Line::from(Span::styled(
                " y/n  Tab=switch  Enter=confirm",
                Style::default().fg(Color::DarkGray),
            ));

            f.render_widget(
                Paragraph::new(vec![Line::from(""), btn_line, Line::from(""), hint]),
                area,
            );
        })?;

        if !event::poll(Duration::from_millis(100))? { continue; }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc | KeyCode::Char('n') => return Ok(()),
                KeyCode::Char('y') => {
                    std::fs::write(RESULT_FILE, "y")?;
                    return Ok(());
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') | KeyCode::Tab => {
                    selected = !selected;
                }
                KeyCode::Enter => {
                    if selected { std::fs::write(RESULT_FILE, "y")?; }
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}
