# How tncli Works

## 1. tncli — Overview

tncli manages multi-repo dev environments via tmux. A project has multiple repos, each repo has multiple services. tncli starts/stops/manages everything from one place.

### Config Loading (every command)

Every command (except `ui`, `update`, `version`, `completion`, `popup`) triggers config loading:

```
root.go:PersistentPreRunE
  → loadConfig()
```

1. `FindConfig()` — walk up from CWD looking for `tncli.yml`
2. `Load(path)` — parse YAML → `Config` struct, extract repo/service ordering, parse custom fields (`env_files`, `shared_services` refs), apply presets
3. `InitNetwork(configDir, session, cfg)`:
   - `RegisterProject(session, projectDir)` — save session→dir mapping in `~/.tncli/registry.json`
   - `ClaimSessionSlot(session)` — lock `~/.tncli/`, load `slots.json`, find free slot (0 or 1), save
   - Build `service_map` from config: `alias~svcName → index` (stable, doesn't change with slot)
   - Build `shared_map` from config: `sharedSvcName → offset` (stable)
   - Save to `.tncli/network.json`

### `tncli start <target>`

```
cmd_start.go:RunE
  → commands.Start(cfg, configPath, target)
```

**Step 1**: Resolve target → list of `[dirName, svcName]` pairs

`cfg.ResolveServices(target)` resolves in order:
1. Check `workspaces` / `combinations` map → expand to service entries
2. Check as repo name → all services of that repo
3. Check as repo alias → all services
4. Check as single service → `FindServiceEntry()` searches by alias/prefix or exact match

**Step 2**: Create tmux session if needed

`tmux.CreateSessionIfNeeded("tncli_<session>")`:
- `tmux has-session -t =tncli_<session>` — check exists
- If not: `tmux new-session -d -s tncli_<session> -n _tncli_init`
- Background goroutine kills `_tncli_init` window after 2s

**Step 3**: For each service, create 1 tmux window

- Skip if window already exists (service already running)
- `cfg.ResolveService(configDir, dirName, svcName)`:
  - Resolve working directory: prefer `workspace--<defaultBranch>/<repo>`, fallback to `<configDir>/<repo>`
  - Inherit env/pre_start from dir if service doesn't define its own
- Build full command: `<env> cd '<workDir>' && <preStart> && <cmd>`
- `tmux.NewWindow(session, svcName, fullCmd)`:
  - `tmux new-window -d -t =<session> -n <svcName> "zsh -ic <fullCmd>; echo '[tncli] process exited...'; read"`
  - Uses `zsh -ic` (interactive) so `.zshrc` loads (nvm, rvm, etc.)
  - Appends exit message + `read` so window stays open after process exits
- `lock.Acquire(session, svcName)` — write lock file to `/tmp/tncli/`

### `tncli stop [target]`

- No target → `tmux kill-session` (kill everything), `lock.ReleaseAll()`
- With target → resolve services, for each:
  - `tmux.GracefulStop(session, window)`:
    1. `tmux send-keys -t =<session>:<window> C-c` — send Ctrl-C
    2. Wait 500ms
    3. `tmux kill-window -t =<session>:<window>` — force kill
  - `lock.Release(session, svcName)` — remove lock file
- If session has no more windows → kill session + release all locks

### `tncli restart <target>`

Simply calls `Stop(target)` then `Start(target)`.

### `tncli status`

- Check `tmux has-session`
- `tmux list-windows -F "#{window_name}"` → set of running window names
- For each repo in config order, print services with `●` (running) or `○` (stopped)

### `tncli attach [service]`

- If service specified: `tmux select-window -t =<session>:<service>`
- Set status-right hint: `Ctrl+b d to return to tncli`
- If already in tmux: `tmux switch-client -t =<session>`
- If not in tmux: `tmux attach-session -t =<session>`
- Restore original status-right after detach

### `tncli logs <service>`

- `tmux.CapturePane(session, service, 100)`:
  - `tmux capture-pane -t =<session>:<service> -e -p -S -100`
  - `-e` preserves ANSI color codes
- Print each line

---

## 2. Port Allocation (on-demand)

Ports are allocated dynamically at runtime — no hardcoded ports, no sudo, no loopback IPs.

### When does allocation happen?

`InitNetwork()` runs on **every config load** (every command). It:
1. Claims a session slot (0 or 1) — max 2 concurrent tncli sessions
2. Builds `service_map` (service → port index within a block) from config
3. Builds `shared_map` (shared service → offset from top of slot) from config
4. Saves to `.tncli/network.json`

When a **workspace is created**, `ClaimBlock()` leases a block for that workspace. When a workspace is **deleted**, `ReleaseBlock()` frees it.

### Port formula

```
Pool: 40000–49999 (10,000 ports total)
Slot 0: 40000–44999 (session A)
Slot 1: 45000–49999 (session B)

Within each slot (5,000 ports):
  Workspace blocks: slotBase + blockIdx × 100 + svcIdx
  Shared services:  slotTop - offset (counting down from top)
```

**Workspace service port**:
```
port = PoolStart + slot × SlotSize + blockIdx × BlockSize + svcIdx
```
Example: slot 0, block 3 (workspace "feat-login"), service index 2:
`40000 + 0×5000 + 3×100 + 2 = 40302`

**Shared service port**:
```
port = PoolStart + slot × SlotSize + SlotSize - 1 - offset
```
Example: slot 0, postgres (offset 0): `44999`

### Conflict avoidance

`ClaimBlock()` skips blocks where the base port is occupied:
```go
if IsPortFree(base + i*BlockSize) { ... }
```
`EnsurePortFree()` auto-reallocates a service to a different offset within its block if the assigned port is taken.

### State files

| File | Scope | Contents |
|------|-------|----------|
| `~/.tncli/slots.json` | Global | Session slot leases (`"0": "myproject"`) |
| `.tncli/network.json` | Per-project | Current slot, workspace blocks, service→index maps |
| `~/.tncli/shared_slots.json` | Global | Capacity-based slot allocations (Redis DB indexes) |

---

## 3. Workspace Create (`tncli workspace create <ws> <branch>`)

```
cmd_workspace.go:RunE
  → commands.WorkspaceCreate(cfg, cfgPath, workspace, branch, fromStage, repos)
```

### Step 0: Build CreateContext

`pipeline.FromConfig(cfg, configPath, wsName, branch, skipStages)`:

1. Lookup workspace name in `cfg.AllWorkspaces()` → list of service entries (e.g. `["api/server", "api/worker", "client/dev"]`)
2. Deduplicate repos → `uniqueDirs` (e.g. `["api", "client"]`)
3. Resolve `dirPaths` — find absolute path for each repo:
   - Prefer `workspace--<defaultBranch>/<repo>` (if exists)
   - Fallback to `<configDir>/<repo>`
4. Resolve `dirBranches` — `git -C <dirPath> rev-parse --abbrev-ref HEAD` → current branch of each repo (used as base branch)
5. Resolve `sharedOverrides` — merge `worktree.disable` + `worktree.shared_services` → docker compose service profiles (mark local copies as `profiles: ["disabled"]`)

Pipeline runner sends events via channel. CLI or TUI reads events for progress display.

```go
ch := make(chan pipeline.Event, 16)
go pipeline.RunCreatePipeline(ctx, ch)
for evt := range ch { ... }
```

### Stage 1/7: Validate

```go
stageValidate(ctx) → nil
```

Currently no-op. Stage kept for future validation logic.

### Stage 2/7: Provision

```go
stageProvision(ctx, state)
```

1. **Allocate shared service slots** — `allocateSharedSlots(ctx)`:
   - For each repo in workspace, scan `worktree.shared_services`:
     - If service has `capacity` → `AllocateSlot(serviceName, wsKey, capacity, basePort)`:
       1. Lock `~/.tncli/slots.lock` (file-based, PID written, 10s stale timeout)
       2. Load `~/.tncli/shared_slots.json`
       3. If already allocated for this wsKey → return existing
       4. Find instance with free slot (slot count < capacity):
          - Track used slots per instance
          - Find first unused slot index
          - Save allocation
       5. If all instances full → create new instance (`InstanceCount++`), assign slot 0
       6. Save → unlock
       7. Returns `(instance, slot, port)` where `port = basePort + instance`
   - Also auto-detect `{{slot:SERVICE}}` patterns in `worktree.env` values

2. **Create workspace folder** — `services.EnsureWorkspaceFolder(configDir, branch)` → `mkdir workspace--<branch>/`

### Stage 3/7: Infra

```go
stageInfra(ctx, state)
```

1. **Generate `docker-compose.shared.yml`** — `GenerateSharedCompose(configDir, session, sharedServices)`:
   - For each shared service in config:
     - Check `MaxInstanceCount(name)` from `shared_slots.json`
     - Generate N service blocks (`redis`, `redis-2`, `redis-3`...)
     - Each instance:
       - Same image, command, environment, healthcheck
       - Port mapping: instance 0 uses config port, instance N uses `hostPort + N`
       - Volume names: instance 0 uses original, instance N appends `-N` suffix
     - Add `restart: unless-stopped`
   - Write volume declarations at bottom
   - Save to `<configDir>/docker-compose.shared.yml`

2. **Start shared containers**:
   ```
   docker compose -f docker-compose.shared.yml -p <session>-shared up -d <all service names>
   ```

3. **Create databases** — `createDatabases(ctx, branchSafe, branch)`:
   - Find postgres service (first shared service with `db_user`)
   - Collect database names from all repos:
     - From `worktree.shared_services[].db_name` templates
     - From `worktree.databases[]` templates
     - Resolve `{{branch_safe}}`, `{{branch}}`
     - Prefix with `session_`
   - `CreateSharedDBsBatch(host, port, dbNames, user, password)`:
     - Find running postgres container: `docker ps -q --filter name=postgres`
     - For each DB: `docker exec <container> psql -U <user> -c 'CREATE DATABASE "<name>"'`
     - If no container found: `docker run --rm postgres:16-alpine psql <connURL> -c ...`
     - Track results: created / exists / failed

### Stage 4/7: Source (parallel)

```go
stageSourceParallel(ctx, state)
```

Launches **1 goroutine per repo**, all run concurrently via `sync.WaitGroup`:

For each repo:
1. `resolveTargetBranch(ctx, dirName)` — use selected branch or default to `ctx.Branch`
2. `services.CreateWorktreeFromBase(dirPath, targetBranch, baseBranch, copyFiles, wsFolder)`:
   - Target directory: `workspace--<branch>/<repo>`
   - Branch resolution:
     - `git show-ref refs/heads/<branch>` → branch exists locally → `git worktree add <path> <branch>`
     - `git ls-remote --heads origin <branch>` → exists on remote → `git fetch origin <branch> && git worktree add --track -b <branch> <path> origin/<branch>`
     - Neither → create new: `git worktree add -b <branch> <path> <baseBranch>`
   - Copy files from repo root to worktree:
     - For each path in `worktree.copy`: `cp -r <repoDir>/<file> <wtDir>/<file>`

3. Collect results in `state.WtDirs` (mutex-protected)

**Partial failure cleanup**: If any goroutine fails → remove ALL already-created worktrees → return first error.

### Stage 5/7: Configure (parallel)

```go
stageConfigureParallel(ctx, state)
```

Launches **1 goroutine per repo**:

For each repo:
1. **Write `.env.tncli`** — `services.WriteEnvFile(wtPath)`:
   - Content: `BIND_IP=127.0.0.1`

2. **Resolve and write env files** — `applyAllEnvFiles(wt, dir, cfg, branch, wsKey)`:
   - Build database names: resolve `{{branch_safe}}` in `databases[]` templates, prefix `session_`
   - Merge env maps: `global cfg.Env` → `worktree.Env` → `per-file entry.Env` (later overrides earlier)
   - `ResolveEnvTemplates(envSrc, cfg, branchSafe, branch, wsKey)`:
     - For each key-value pair:
       1. `{{bind_ip}}` → `127.0.0.1`
       2. `{{branch_safe}}` → branch with `/` and `-` replaced by `_`
       3. `{{branch}}` → raw branch name
       4. `{{slot:SERVICE}}` → allocated slot index from `shared_slots.json`
       5. `{{host:NAME}}` → `SharedServices[NAME].Host` or `127.0.0.1`
       6. `{{port:NAME}}` → first port from `SharedServices[NAME].Ports`, or `Repos[NAME].ProxyPort`
       7. `{{url:NAME}}` → `http://<host>:<port>`
       8. `{{conn:NAME}}` → `<db_user>:<db_password>@<host>:<port>`
   - `{{db:N}}` → Nth database name (session-prefixed)
   - `ApplyEnvOverrides(dir, resolved, entryFile)` — write to `.env.local`, `.env.development.local`, etc.

3. **Ensure global gitignore** — add `docker-compose.override.yml`, `.env.tncli`, `.env.local` to global gitignore

4. **Ensure node-bind-host.js** — `~/.tncli/node-bind-host.js` monkey-patches Node.js `net.Server.listen` to respect `BIND_IP`

### Stage 6/7: Setup (parallel via tmux)

```go
stageSetupParallel(ctx, state)
```

1. **Create tmux session** if needed

2. For each repo with `setup` commands:
   - Join all commands: `"npm install && npx prisma generate && npx prisma migrate deploy"`
   - Build NODE_OPTIONS with bind-host patch if exists
   - Create tmux window:
     ```
     tmux new-window -n "setup~<alias>~<branchSafe>"
       "cd '<wtPath>' && set -a && source .env.local; set +a && <NODE_OPTIONS> && <combined_commands>"
     ```
   - Set `remain-on-exit on` so window stays visible after command finishes

3. **Poll until all setup windows finish** — `waitForSetupWindows(session, windows)`:
   - Every 2 seconds: check `tmux list-panes -F "#{pane_dead}"` per window
   - When ALL windows dead → kill all setup windows

### Stage 7/7: Network

```go
stageNetworkCreate(ctx, state)
```

1. **Create Docker network**: `docker network create tncli-ws-<branch>`

### Result

```
workspace--<branch>/
  ├── repo-a/                    ← git worktree, env resolved
  │   ├── docker-compose.override.yml  ← generated
  │   ├── .env.tncli                   ← BIND_IP=127.0.0.1
  │   └── .env.local                   ← resolved templates
  ├── repo-b/                    ← git worktree, env resolved
  └── ...

State persisted:
  ~/.tncli/shared_slots.json     ← slot allocations (Redis DB indexes)
  ~/.tncli/slots.json            ← session slot lease (0 or 1)
  .tncli/network.json            ← port allocations (blocks, service_map, shared_map)
  ~/.tncli/registry.json         ← session → project dir mapping
  ~/.tncli/pipeline-<branch>.json ← pipeline state (for --from-stage resume)
  N databases created in shared postgres
  Docker network tncli-ws-<branch> created
```

### Resume on Failure

```
tncli workspace create <ws> <branch> --from-stage 4
```

Skips stages 1-3 (already completed), resumes from stage 4 (Source).

---

## 4. Workspace Delete (`tncli workspace delete <branch>`)

```
cmd_workspace.go:RunE
  → commands.WorkspaceDelete(cfg, cfgPath, branch)
```

### Step 0: Build DeleteContext

1. For each repo in config:
   - Resolve repo dir path (prefer `workspace--<defaultBranch>/<repo>`)
   - Check if `workspace--<branch>/<repo>` exists
   - If exists → add to `cleanupItems` with `pre_delete` commands
2. Collect `dbsToDrop`:
   - From `worktree.shared_services[].db_name` templates → resolve `{{branch_safe}}`
   - From `worktree.databases[]` templates → resolve `{{branch_safe}}`, prefix `session_`
3. Set network name: `tncli-ws-<branch>`

### Stage 1/5: Stop

No-op. Caller is expected to stop services before calling delete.

### Stage 2/5: Release

```go
deleteStageRelease(ctx)
```

1. **Release shared service slots** — for each shared service in config:
   - `ReleaseSlot(serviceName, wsKey)`:
     1. Lock `~/.tncli/slots.lock`
     2. Load `shared_slots.json`
     3. Delete entry for `wsKey`
     4. Shrink `InstanceCount` if last instance now empty
     5. Save → unlock

### Stage 3/5: Cleanup

```go
deleteStageCleanup(ctx)
```

1. For each repo with `pre_delete` commands:
   - Run: `zsh -c "<combined>"` in worktree dir
   - Errors ignored (best-effort cleanup)

### Stage 4/5: Remove

```go
deleteStageRemove(ctx)
```

1. **Remove git worktrees** — for each repo:
   - `git -C <dirPath> worktree remove <wtPath>` (force if needed)
   - `git -C <dirPath> branch -D <branch>` (delete local branch)

2. **Drop databases** — for each database:
   - Terminate active connections: `SELECT pg_terminate_backend(...)`
   - Drop: `DROP DATABASE IF EXISTS "<db>"`
   - Runs via `docker exec` or `docker run`

3. **Release port block** — `ReleaseBlock(projectDir, wsKey)`:
   - Lock project dir, load `network.json`, delete block entry, save, unlock

### Stage 5/5: Finalize

```go
deleteStageFinalize(ctx)
```

1. **Remove Docker network**: `docker network rm tncli-ws-<branch>`
2. **Delete workspace folder**: `os.RemoveAll(workspace--<branch>/)`

### Result

Everything cleaned up:
- `workspace--<branch>/` — deleted
- Git worktrees — removed, local branches deleted
- Databases — dropped (connections terminated first)
- Port blocks — released from `network.json`
- Shared service slots — freed from `shared_slots.json`
- Docker network — removed
- Pipeline state files — cleared
