# pipeline/

He thong pipeline tao va xoa workspace theo tung stage, ho tro resume khi bi loi.

## Files

| File | Chuc nang |
|---|---|
| `mod.rs` | Dinh nghia `PipelineEvent` enum (StageStarted, StageCompleted, StageSkipped, PipelineCompleted, PipelineFailed). Quan ly state persistence tai `~/.tncli/pipeline-{branch}.json` va `~/.tncli/active/{branch}` |
| `stages.rs` | Dinh nghia `CreateStage` (7 stage) va `DeleteStage` (5 stage) voi label mo ta |
| `create.rs` | Logic tao workspace: Validate → Provision → Infra → Source → Configure → Setup → Network. Ho tro `--from-stage` de resume va `--repos` de chon repo cu the |
| `delete.rs` | Logic xoa workspace: Stop → Release → Cleanup → Remove → Finalize |
| `context.rs` | Builder tao `PipelineContext` tu Config, chua thong tin branch, repos, IP, slots can thiet cho pipeline |

## Pipeline Create (7 stages)

1. **Validate** — kiem tra config hop le, branch chua ton tai, `/etc/hosts` dung
2. **Provision** — cap phat IP loopback (`127.0.X.Y`) va slot cho shared services
3. **Infra** — khoi dong shared services (Docker containers), tao database
4. **Source** — tao git worktree song song cho moi repo
5. **Configure** — generate docker-compose override, env files, copy template files
6. **Setup** — chay setup commands (migrations, seed...) song song
7. **Network** — tao Docker network cho workspace

## Pipeline Delete (5 stages)

1. **Stop** — dung tat ca service dang chay
2. **Release** — giai phong IP va slot
3. **Cleanup** — chay `pre_delete` commands
4. **Remove** — xoa git worktree, drop database
5. **Finalize** — xoa Docker network va thu muc

## State Persistence

- `~/.tncli/pipeline-{branch}.json` — luu stage hien tai de resume
- `~/.tncli/active/{branch}` — danh dau pipeline dang chay
- `list_active_pipelines()` — liet ke cac pipeline dang chay do
