# services/

Infrastructure layer — xu ly tat ca tac vu he thong: Docker, git, mang, DNS, file, proxy.

## Files

| File | Chuc nang |
|---|---|
| `mod.rs` | Re-export cac module. Chua cac ham resolve template: `resolve_env_templates()`, `resolve_config_templates()`, `resolve_db_templates()`, `resolve_slot_templates()`, `branch_safe()` |
| `docker.rs` | Docker integration: `docker_force_cleanup()`, `create_docker_network()`, `remove_docker_network()`, `ensure_workspace_folder()`, `delete_workspace_folder()`, `ensure_main_workspace()` |
| `git.rs` | Git worktree: `list_worktrees()`, `is_branch_in_worktree()`, `create_worktree_from_base()`, `create_worktree()`, `remove_worktree()` |
| `ip.rs` | IP allocation: cap phat IP loopback `127.0.{subnet}.{host}` cho moi workspace. State tai `~/.tncli/network.json`. Ham: `allocate_ip()`, `release_ip()`, `main_ip()`, `check_etc_hosts()` |
| `dns.rs` | DNS/dnsmasq setup: `is_dnsmasq_installed()`, `is_dnsmasq_configured()`, `setup_dnsmasq()`, `verify_resolution()`. Cau hinh wildcard `*.tncli.test → 127.0.0.1` |
| `compose.rs` | Docker Compose: `generate_compose_override()` tao docker-compose.override.yml voi BIND_IP, `setup_main_as_worktree()` |
| `files.rs` | File management: `apply_env_overrides()`, `write_env_file()`, `copy_files()`, `ensure_global_gitignore()`, `ensure_node_bind_host()` |
| `workspace.rs` | Shared services: `generate_shared_compose()`, `start_shared_services()`, `create_shared_dbs_batch()`, `drop_shared_dbs_batch()`, `allocate_slot()`, `release_slot()` |
| `proxy.rs` | Reverse proxy: `run_proxy_server()` (foreground), `register_routes()`, `load_routes()`, `save_routes()`, `proxy_hostname()`. Routes tai `~/.tncli/proxy-routes.json` |

## Template Variables

| Template | Giai thich |
|---|---|
| `{{bind_ip}}` | IP loopback cua workspace |
| `{{branch_safe}}` | Branch name da sanitize (`/`,`-` → `_`) |
| `{{host:name}}` | Hostname cua service/repo |
| `{{port:name}}` | Port cua service/repo |
| `{{url:name}}` | `http://{host}:{port}` |
| `{{conn:name}}` | `user:pass@host:port` (shared service) |
| `{{db:N}}` | Ten database thu N (auto-prefixed) |
| `{{slot:name}}` | Slot index cho capacity-limited service |

## State Files

- `~/.tncli/network.json` — IP allocations (version 2: subnets + allocations)
- `~/.tncli/slots.json` — slot allocations cho shared services
- `~/.tncli/proxy-routes.json` — routing table (hostname:port → bind_ip:port)
