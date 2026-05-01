use crate::tui::app::{App, ComboItem, workspace_branch, save_collapse_state};

impl App {
    /// Build flattened Workspaces tree: combo definitions + active instances nested under them.
    pub fn rebuild_combo_tree(&mut self) {
        self.combo_items.clear();

        // Collect active workspace instances (grouped by branch, sorted by name)
        let mut instances: indexmap::IndexMap<String, Vec<(String, String)>> = indexmap::IndexMap::new();
        for (wt_key, wt) in &self.worktrees {
            if let Some(branch) = workspace_branch(wt) {
                instances.entry(branch).or_default().push((wt.parent_dir.clone(), wt_key.clone()));
            }
        }
        instances.sort_keys();
        let all_ws = self.config.all_workspaces();

        // Sort dirs within each instance by combo order (not alphabetical)
        for (_, dirs) in instances.iter_mut() {
            let combo_order: Vec<String> = all_ws.values().flat_map(|entries| {
                let mut seen = Vec::new();
                for entry in entries {
                    if let Some((dir, _)) = self.config.find_service_entry_quiet(entry) {
                        if !seen.contains(&dir) { seen.push(dir); }
                    }
                }
                seen
            }).collect();
            dirs.sort_by(|a, b| {
                let ia = combo_order.iter().position(|d| d == &a.0).unwrap_or(usize::MAX);
                let ib = combo_order.iter().position(|d| d == &b.0).unwrap_or(usize::MAX);
                ia.cmp(&ib)
            });
        }
        let mut matched_instances: std::collections::HashSet<String> = std::collections::HashSet::new();

        for name in &self.combos {
            self.combo_items.push(ComboItem::Combo(name.clone()));

            // Find combo's unique dir names
            let combo_dirs: Vec<String> = all_ws.get(name)
                .map(|entries| {
                    let mut dirs = Vec::new();
                    for entry in entries {
                        if let Some((dir, _)) = self.config.find_service_entry_quiet(entry) {
                            if !dirs.contains(&dir) { dirs.push(dir); }
                        }
                    }
                    dirs
                })
                .unwrap_or_default();

            // "main" instance (always first, virtual)
            let default_branch = self.config.global_default_branch().to_string();
            let main_inst_key = format!("ws-inst-main-{name}");
            self.combo_items.push(ComboItem::Instance { branch: default_branch.clone(), is_main: true });
            if !self.combo_collapsed.get(&main_inst_key).copied().unwrap_or(false) {
                for dir_name in &combo_dirs {
                    self.combo_items.push(ComboItem::InstanceDir {
                        branch: default_branch.clone(),
                        dir: dir_name.clone(),
                        wt_key: String::new(),
                        is_main: true,
                    });
                    let dir_key = format!("ws-dir-main-{name}-{dir_name}");
                    let all_svcs = self.config.all_services_for(dir_name);
                    let svc_count = all_svcs.len();
                    if svc_count > 1 && !self.combo_collapsed.get(&dir_key).copied().unwrap_or(false) {
                        let alias = self.config.repos.get(dir_name)
                            .and_then(|d| d.alias.as_deref())
                            .unwrap_or(dir_name);
                        for svc_name in &all_svcs {
                            let tmux_name = format!("{alias}~{svc_name}");
                            self.combo_items.push(ComboItem::InstanceService {
                                branch: default_branch.clone(),
                                dir: dir_name.clone(),
                                wt_key: String::new(),
                                svc: svc_name.clone(),
                                tmux_name,
                                is_main: true,
                            });
                        }
                    }
                }
                // Add worktree-level global services at instance level (same as repos)
                for (svc_name, _) in self.config.worktree_level_services() {
                    let tmux_name = format!("_global~{svc_name}");
                    self.combo_items.push(ComboItem::InstanceService {
                        branch: default_branch.clone(),
                        dir: String::new(),
                        wt_key: String::new(),
                        svc: svc_name.clone(),
                        tmux_name,
                        is_main: true,
                    });
                }
            }

            // Find instances whose dirs match this combo
            for (branch, dirs) in &instances {
                if matched_instances.contains(branch) { continue; }
                let inst_dirs: Vec<&str> = dirs.iter().map(|(d, _)| d.as_str()).collect();
                let matches = !combo_dirs.is_empty()
                    && combo_dirs.iter().all(|d| inst_dirs.contains(&d.as_str()));
                if !matches { continue; }

                matched_instances.insert(branch.clone());
                self.combo_items.push(ComboItem::Instance { branch: branch.clone(), is_main: false });

                let inst_key = format!("ws-inst-{branch}");
                if !self.combo_collapsed.get(&inst_key).copied().unwrap_or(false) {
                    for (dir_name, wt_key) in dirs {
                        self.combo_items.push(ComboItem::InstanceDir {
                            branch: branch.clone(),
                            dir: dir_name.clone(),
                            wt_key: wt_key.clone(),
                            is_main: false,
                        });

                        let dir_key = format!("ws-dir-{branch}-{dir_name}");
                        let all_svcs = self.config.all_services_for(dir_name);
                        let svc_count = all_svcs.len();
                        if svc_count > 1 && !self.combo_collapsed.get(&dir_key).copied().unwrap_or(false) {
                            for svc_name in &all_svcs {
                                let tmux_name = self.wt_tmux_name(dir_name, svc_name, branch);
                                self.combo_items.push(ComboItem::InstanceService {
                                    branch: branch.clone(),
                                    dir: dir_name.clone(),
                                    wt_key: wt_key.clone(),
                                    svc: svc_name.clone(),
                                    tmux_name,
                                    is_main: false,
                                });
                            }
                        }
                    }
                    // Add worktree-level global services at instance level
                    for (svc_name, _) in self.config.worktree_level_services() {
                        let branch_safe = crate::services::branch_safe(branch);
                        let tmux_name = format!("_global~{svc_name}~{branch_safe}");
                        self.combo_items.push(ComboItem::InstanceService {
                            branch: branch.clone(),
                            dir: String::new(),
                            wt_key: String::new(),
                            svc: svc_name.clone(),
                            tmux_name,
                            is_main: false,
                        });
                    }
                }
            }

            // Show creating workspaces AFTER existing instances (stable order)
            for branch in &self.creating_workspaces.clone() {
                if !matched_instances.contains(branch) && !instances.contains_key(branch) {
                    self.combo_items.push(ComboItem::Instance { branch: branch.clone(), is_main: false });
                    matched_instances.insert(branch.clone());
                }
            }
        }

        // Orphan instances (no matching combo) — show at end
        for (branch, dirs) in &instances {
            if matched_instances.contains(branch) { continue; }
            self.combo_items.push(ComboItem::Instance { branch: branch.clone(), is_main: false });
            let inst_key = format!("ws-inst-{branch}");
            if !self.combo_collapsed.get(&inst_key).copied().unwrap_or(false) {
                for (dir_name, wt_key) in dirs {
                    self.combo_items.push(ComboItem::InstanceDir {
                        branch: branch.clone(),
                        dir: dir_name.clone(),
                        wt_key: wt_key.clone(),
                        is_main: false,
                    });
                    let dir_key = format!("ws-dir-{branch}-{dir_name}");
                    let all_svcs = self.config.all_services_for(dir_name);
                    let svc_count = all_svcs.len();
                    if svc_count > 1 && !self.combo_collapsed.get(&dir_key).copied().unwrap_or(false) {
                        for svc_name in &all_svcs {
                            let tmux_name = self.wt_tmux_name(dir_name, svc_name, branch);
                            self.combo_items.push(ComboItem::InstanceService {
                                branch: branch.clone(),
                                dir: dir_name.clone(),
                                wt_key: wt_key.clone(),
                                svc: svc_name.clone(),
                                tmux_name,
                                is_main: false,
                            });
                        }
                    }
                }
                // worktree-level global services
                for (svc_name, _) in self.config.worktree_level_services() {
                    let branch_safe = crate::services::branch_safe(branch);
                    let tmux_name = format!("_global~{svc_name}~{branch_safe}");
                    self.combo_items.push(ComboItem::InstanceService {
                        branch: branch.clone(),
                        dir: String::new(),
                        wt_key: String::new(),
                        svc: svc_name.clone(),
                        tmux_name,
                        is_main: false,
                    });
                }
            }
        }

        // Clamp cursor after rebuild
        let max = self.combo_items.len();
        if max > 0 && self.cursor >= max {
            self.cursor = max - 1;
        }
    }

    /// Toggle collapse of a workspace instance/dir at cursor.
    pub fn toggle_collapse(&mut self) {
        match self.combo_items.get(self.cursor).cloned() {
            Some(ComboItem::Instance { branch, is_main }) => {
                // For main instance, use combo-specific key; find parent combo
                let key = if is_main {
                    // Find the combo name above this item
                    let combo_name = self.find_parent_combo(self.cursor);
                    format!("ws-inst-main-{combo_name}")
                } else {
                    format!("ws-inst-{branch}")
                };
                let collapsed = self.combo_collapsed.get(&key).copied().unwrap_or(false);
                self.combo_collapsed.insert(key, !collapsed);
                self.rebuild_combo_tree();
            }
            Some(ComboItem::InstanceDir { branch, dir, is_main, .. }) => {
                // Don't allow collapse for dirs with only 1 service
                let svc_count = self.config.all_services_for(&dir).len();
                if svc_count <= 1 { return; }
                let key = if is_main {
                    let combo_name = self.find_parent_combo(self.cursor);
                    format!("ws-dir-main-{combo_name}-{dir}")
                } else {
                    format!("ws-dir-{branch}-{dir}")
                };
                let collapsed = self.combo_collapsed.get(&key).copied().unwrap_or(false);
                self.combo_collapsed.insert(key, !collapsed);
                self.rebuild_combo_tree();
            }
            _ => {}
        }
        self.save_collapse_state();
    }

    /// Find the parent Combo name for an item at a given index.
    pub(crate) fn find_parent_combo(&self, idx: usize) -> String {
        for i in (0..=idx).rev() {
            if let Some(ComboItem::Combo(name)) = self.combo_items.get(i) {
                return name.clone();
            }
        }
        String::new()
    }

    /// Save collapse state to disk.
    fn save_collapse_state(&self) {
        save_collapse_state(&self.session, &self.dir_names, &self.wt_collapsed, &self.combo_collapsed);
    }

    /// Scan for existing git worktrees and load them.
    pub fn scan_worktrees(&mut self) {
        self.worktrees.clear();
        for dir_name in &self.dir_names {
            // Scan all dirs (workspace can create worktrees for dirs without worktree: true)
            let dir_path = match self.dir_path(dir_name) {
                Some(p) => p,
                None => continue,
            };
            let wts = match crate::services::list_worktrees(std::path::Path::new(&dir_path)) {
                Ok(w) => w,
                Err(_) => continue,
            };
            // Skip the main worktree (first entry = the repo itself)
            for (wt_path, branch) in wts.iter().skip(1) {
                let wt_path = std::path::PathBuf::from(wt_path);
                let wt_key = format!("{dir_name}--{}", branch.replace('/', "-"));
                let allocs = crate::services::load_ip_allocations();
                // Try per-worktree key first, then workspace key (ws-{branch})
                let ip = allocs.get(&wt_key)
                    .or_else(|| allocs.get(&format!("ws-{}", branch.replace('/', "-"))))
                    // Fallback: extract workspace branch from parent folder name (workspace--{branch})
                    .or_else(|| {
                        wt_path.parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_string_lossy().strip_prefix("workspace--").map(|s| format!("ws-{}", s)))
                            .and_then(|key| allocs.get(&key))
                    })
                    .cloned()
                    .unwrap_or_default();
                self.worktrees.insert(wt_key, crate::services::WorktreeInfo {
                    branch: branch.clone(),
                    parent_dir: dir_name.clone(),
                    bind_ip: ip,
                    path: wt_path,
                });
            }
        }
        self.rebuild_combo_tree();
    }
}
