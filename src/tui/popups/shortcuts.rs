use crate::tmux;
use super::super::app::{App, ComboItem, PendingPopup, POPUP_RESULT_FILE};

impl App {
    pub fn popup_shortcuts(&mut self) {
        let item = match self.current_combo_item().cloned() {
            Some(i) => i,
            None => { self.set_message("no shortcuts for this item"); return; }
        };
        let (items, title) = match item {
            ComboItem::InstanceDir { ref dir, .. } => {
                let dir_obj = match self.config.repos.get(dir) {
                    Some(d) => d,
                    None => return,
                };
                if dir_obj.shortcuts.is_empty() {
                    self.set_message(&format!("no shortcuts for dir '{dir}'"));
                    return;
                }
                (dir_obj.shortcuts.clone(), dir.clone())
            }
            ComboItem::InstanceService { ref dir, ref svc, .. } => {
                let dir_obj = match self.config.repos.get(dir) {
                    Some(d) => d,
                    None => return,
                };
                let svc_obj = match dir_obj.services.get(svc) {
                    Some(s) => s,
                    None => return,
                };
                let mut merged = dir_obj.shortcuts.clone();
                merged.extend(svc_obj.shortcuts.clone());
                if merged.is_empty() {
                    self.set_message("no shortcuts");
                    return;
                }
                (merged, format!("{dir}/{svc}"))
            }
            _ => { self.set_message("no shortcuts for this item"); return; }
        };

        self.shortcuts_items = items.clone();
        self.shortcuts_title = title;

        let _ = std::fs::remove_file(POPUP_RESULT_FILE);
        let lines: Vec<String> = items.iter().enumerate()
            .map(|(i, s)| format!("{}\t{} -> {}", i, s.desc, s.cmd))
            .collect();
        let input = lines.join("\n");
        let cmd = format!(
            "echo '{}' | fzf --prompt='Shortcut> ' --with-nth=2.. --delimiter='\t' | cut -f1 > {}",
            input.replace('\'', "'\\''"), POPUP_RESULT_FILE
        );
        tmux::display_popup("70%", "50%", &cmd);
        self.pending_popup = Some(PendingPopup::Shortcut);
    }

    pub fn popup_cheatsheet(&mut self) {
        let content = r#"
  Left Panel
  j/k          Navigate up/down
  Enter/Space  Toggle start/stop or collapse
  s            Start service/instance
  x            Stop service/instance
  X            Stop all (confirm)
  r            Restart
  c            Shortcuts popup
  e            Open in editor
  g            Git: checkout/pull/diff (main: pull+diff only)
  w            Create workspace / worktree menu
  d            Delete workspace (confirm)
  t            Shell in popup
  I            Shared services info
  R            Reload config
  Tab/l        Focus service pane
  n/N          Cycle running services

  Global
  ?            This cheat-sheet
  q            Quit
"#;
        let cmd = format!(
            "echo '{}' | less -R --prompt='Keybindings (q to close)'",
            content.replace('\'', "'\\''")
        );
        tmux::display_popup("50%", "70%", &cmd);
    }

    pub fn popup_shared_info(&mut self) {
        if self.config.shared_services.is_empty() {
            self.set_message("no shared services configured");
            return;
        }
        let session = &self.session;
        let project = format!("{session}-shared");
        let mut lines = Vec::new();
        lines.push(format!("  Shared Services ({})", project));
        lines.push(String::new());
        for (name, svc) in &self.config.shared_services {
            let host = svc.host.as_deref().unwrap_or("-");
            let ports: String = svc.ports.iter()
                .map(|p| p.split(':').next().unwrap_or(p).to_string())
                .collect::<Vec<_>>().join(", ");
            let cap = svc.capacity.map(|c| format!(" (cap:{c})")).unwrap_or_default();
            lines.push(format!("  {name:<16} {host:<22} :{ports}{cap}"));
        }
        let content = lines.join("\n");
        let cmd = format!(
            "echo '{}' | less -R --prompt='Shared Services (q to close)'",
            content.replace('\'', "'\\''")
        );
        tmux::display_popup("60%", "50%", &cmd);
    }

    pub fn run_shortcut_in_popup(&mut self, cmd: &str, desc: &str, dir: &str) {
        let log = "/tmp/tncli-shortcut-output.log";
        let script = format!(
            "#!/bin/zsh\nLOG='{}'\ncd '{}'\n({}) 2>&1 | tee \"$LOG\"\nless -R --mouse +G \"$LOG\"\nrm -f \"$LOG\"\n",
            log, dir, cmd
        );
        let script_path = "/tmp/tncli-shortcut-run.sh";
        let _ = std::fs::write(script_path, &script);
        let _ = std::process::Command::new("chmod").args(["+x", script_path]).output();
        tmux::display_popup("80%", "80%", script_path);
        self.set_message(&format!("running: {desc}"));
    }
}

pub fn handle(app: &mut App, _popup: PendingPopup, result: Option<String>) {
    if let Some(idx_str) = result {
        if let Ok(idx) = idx_str.parse::<usize>() {
            app.shortcuts_cursor = idx;
            if let Some((cmd, desc, dir)) = app.selected_shortcut() {
                app.run_shortcut_in_popup(&cmd, &desc, &dir);
            }
        }
    }
}
