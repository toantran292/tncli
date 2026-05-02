# Pipeline Create Flow

Creates a complete development environment from a branch name — worktrees, databases, IPs, proxy routes, env files.

## Entry Point

```
tncli workspace create <workspace> <branch> [--from-stage N] [--repos r1:b1,r2:b2]
```

```
cmd/tncli/main.go → commands.WorkspaceCreate()
  → pipeline.FromConfig() → build CreateContext
  → pipeline.RunCreatePipeline(ctx, channel) ← goroutine
  → 7 stages sequential, events sent via channel
```

## Stage 1: Validate

Check `/etc/hosts` has entries for shared services with non-`*.tncli.test` hostnames.
dnsmasq handles `*.tncli.test` wildcard — only custom hostnames need `/etc/hosts`.

Fail → `"Run: tncli setup"`

## Stage 2: Provision

1. **Allocate IP** — `AllocateIP(session, "ws-<branch>")` → `127.0.{subnet}.{host}` (file-locked, atomic via `network.json`)
2. **Allocate shared service slots** — scan config for capacity-limited services (e.g., Redis with 16 DB indexes). Also auto-detect `{{slot:SERVICE}}` patterns in env values.
3. **Create workspace folder** — `workspace--<branch>/`

## Stage 3: Infra

1. **Generate** `docker-compose.shared.yml` from `shared_services` config
2. **Start shared containers** — `docker compose up -d` (postgres, redis, minio, etc.)
3. **Create databases** — for each repo's `databases:` field, resolve templates and batch-create via `docker exec` into postgres container:
   - `{session}_{branch_safe}` (e.g., `tncli_feature_x`)
   - `{session}_transaction_{branch_safe}`
   - Legacy: `shared_services[].db_name` template

## Stage 4: Source (parallel)

One goroutine per repo, all run concurrently:

```
git -C <repo_dir> worktree add workspace--<branch>/<repo> -b <branch> <base_branch>
```

Branch resolution:
- Branch exists locally → `git worktree add ... <branch>`
- Branch exists on origin → `git worktree add --track -b <branch> ... origin/<branch>`
- Neither → create new branch from base: `git worktree add -b <branch> ... <base_branch>`

After git → copy configured files (`.env`, `vendor/`, etc.) from main repo to worktree.

**Partial failure cleanup**: if any goroutine fails, all already-created worktrees are removed before returning error.

## Stage 5: Configure (parallel)

One goroutine per repo:

1. **Write `.env.tncli`** — `BIND_IP=<allocated_ip>`
2. **Resolve and write env files** — for each `env_files` entry:
   - Merge: global `env` → worktree `env` → per-file `env` (later wins)
   - Resolve templates in order:
     - `{{bind_ip}}` → allocated IP
     - `{{branch_safe}}` → branch with `/` and `-` → `_`
     - `{{slot:SERVICE}}` → allocated slot index
     - `{{host:NAME}}` → shared service hostname or repo proxy hostname
     - `{{port:NAME}}` → shared service port or repo proxy_port
     - `{{url:NAME}}` → `http://{host}:{port}`
     - `{{conn:NAME}}` → `user:pass@host:port`
     - `{{db:N}}` → Nth database name (session-prefixed)
   - Write to configured env file (e.g., `.env.local`, `.env.development.local`)
   - Only override keys that exist in other `.env*` files (smart merge)

3. **Ensure global gitignore** — add `docker-compose.override.yml`, `.env.tncli`, `.env.local`, etc.
4. **Ensure node-bind-host.js** — DNS patch + BIND_IP monkey-patch for Node.js

## Stage 6: Setup (parallel via tmux)

Each repo's `setup` commands (or inherited from `preset`) run in tmux windows:

```
tmux new-window -n "setup~<alias>~<branch_safe>" \
  "cd '<worktree_path>' && source .env.local && <NODE_OPTIONS> && <setup_commands>"
```

- `remain-on-exit` set so window stays visible after command finishes
- Poll every 2s checking `#{pane_dead}` for each window
- All done → kill all setup windows at once

Example setup commands (from presets):
```yaml
setup:
  - docker compose down -v --remove-orphans || true
  - npm install
  - npx prisma generate
  - npx prisma migrate deploy
```

## Stage 7: Network

1. **Create Docker network** — `docker network create tncli-ws-<branch>`
2. **Register proxy routes** — for each repo with `proxy_port`:
   ```
   {session}.{alias}.ws-{branch_safe}.tncli.test:{port} → {bind_ip}:{port}
   ```
3. **Reload Caddy** — regenerate Caddyfile from `proxy-routes.json`, then `caddy reload`

## Result

```
workspace--<branch>/
  repo-a/          ← git worktree, allocated IP, env resolved
  repo-b/          ← git worktree, allocated IP, env resolved
  ...

State created:
  ~/.tncli/network.json    — IP allocation recorded
  ~/.tncli/shared_slots.json — slot allocations recorded
  ~/.tncli/proxy-routes.json — proxy routes registered
  N databases created in shared postgres
  Docker network created
  Caddy routes active
```

## Resume on Failure

```
tncli workspace create <ws> <branch> --from-stage 4
```

Skips stages 1-3 (already completed), resumes from stage 4 (Source).

State files:
- `~/.tncli/pipeline-<branch>.json` — which stage failed and error message
- `~/.tncli/active/<branch>` — marker that pipeline is running (stage/total/label)

## Corresponding Delete Flow

See `pipeline/delete.go` — 5 stages:
1. **Stop** — no-op (caller handles)
2. **Release** — release shared service slots
3. **Cleanup** — run `pre_delete` commands
4. **Remove** — remove git worktrees, drop databases, release IP
5. **Finalize** — remove Docker network, delete workspace folder, unregister proxy routes
