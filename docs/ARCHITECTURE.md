# Architecture

Go single-binary CLI+TUI for managing multi-repo dev environments via tmux.

## Layer Diagram

```mermaid
graph TD
    CONFIG[tncli.yml<br/>session, repos, services,<br/>shared_services, combinations]

    CONFIG --> ROOT[cmd/tncli/root.go<br/>cobra dispatch + config load<br/>+ InitNetwork]

    ROOT --> CLI[CLI<br/>commands/]
    ROOT --> TUI[TUI<br/>tui/ bubbletea]
    ROOT --> PIPE[Pipeline<br/>pipeline/]

    CLI --> SVC[services/<br/>network, compose, docker,<br/>git, files, workspace,<br/>registry, templates]
    TUI --> SVC
    PIPE --> SVC

    SVC --> TMUX[tmux/<br/>session, window, pane<br/>capture, attach, send-keys]
    SVC --> PATHS[paths/<br/>XDG state directory<br/>~/.local/state/tncli/]

    CLI -.-> TMUX
    TUI -.-> TMUX
    PIPE -.-> TMUX

    style CONFIG fill:#f9f,stroke:#333
    style ROOT fill:#bbf,stroke:#333
    style CLI fill:#bfb,stroke:#333
    style TUI fill:#bfb,stroke:#333
    style PIPE fill:#bfb,stroke:#333
    style SVC fill:#fdb,stroke:#333
    style TMUX fill:#ddd,stroke:#333
    style PATHS fill:#ddd,stroke:#333
```

## Data Flow

```mermaid
flowchart LR
    YML[tncli.yml] --> PARSE[Parse Config]
    PARSE --> INIT[InitNetwork]
    INIT --> SLOT[ClaimSessionSlot<br/>slots.json]
    INIT --> SMAP[Build service_map<br/>+ shared_map<br/>network.json]

    SMAP --> RESOLVE[Template Resolution]
    RESOLVE --> ENV[.env.local<br/>host processes]
    RESOLVE --> COMPOSE[docker-compose.override.yml<br/>Docker containers]
    RESOLVE --> SHARED[docker-compose.shared.yml<br/>shared services]

    style YML fill:#f9f
    style RESOLVE fill:#fdb
    style ENV fill:#bfb
    style COMPOSE fill:#bfb
    style SHARED fill:#bfb
```

## Port Allocation

```mermaid
block-beta
    columns 1
    block:POOL["Port Pool: 40000-49999 (10,000 ports)"]
        columns 2
        block:S0["Slot 0: 40000-44999"]
            columns 1
            WS0["Workspace blocks<br/>40000-44799<br/>48 blocks x 100 ports"]
            SH0["Shared services<br/>44800-44999<br/>200 ports reserved"]
        end
        block:S1["Slot 1: 45000-49999"]
            columns 1
            WS1["Workspace blocks<br/>45000-49799"]
            SH1["Shared services<br/>49800-49999"]
        end
    end

    style WS0 fill:#bfb
    style WS1 fill:#bfb
    style SH0 fill:#fdb
    style SH1 fill:#fdb
```

Formulas:
- **Workspace service**: `PoolStart + slot * SlotSize + blockIdx * BlockSize + svcIdx`
- **Shared service**: `PoolStart + slot * SlotSize + SlotSize - SharedReserve + offset`
- **Multi-port**: consecutive offsets per service
- **Multi-instance** (capacity): `SharedPort(name) + instanceIdx`

## Networking

```mermaid
flowchart TD
    subgraph HOST["Host Machine"]
        BROWSER[Browser]
        HOSTPROC[Host Process<br/>npm, vite, etc.]
        HOSTS["/etc/hosts<br/>127.0.0.1 postgres<br/>127.0.0.1 redis<br/>127.0.0.1 minio"]
        HOSTPORT["Host Port Mapping<br/>44800 -> :5432<br/>44801 -> :6379"]
    end

    subgraph DOCKER["Docker"]
        CONTAINER[Workspace Container<br/>api via dip]
        EXTRA["extra_hosts:<br/>postgres:host-gateway<br/>host.docker.internal:host-gateway"]
        subgraph SHARED["tncli-shared network"]
            PG[postgres :5432]
            REDIS[redis :6379]
            MINIO[minio :9000]
        end
    end

    BROWSER -->|"postgres:44800"| HOSTS
    HOSTS -->|"127.0.0.1:44800"| HOSTPORT
    HOSTPORT --> PG

    HOSTPROC -->|"postgres:44800"| HOSTS

    CONTAINER -->|"postgres:44800"| EXTRA
    EXTRA -->|"host-gateway:44800"| HOSTPORT

    CONTAINER -->|"host.docker.internal:17002"| EXTRA
    EXTRA -->|"host-gateway:17002"| HOSTPROC

    style BROWSER fill:#f9f
    style CONTAINER fill:#bbf
    style PG fill:#bfb
    style REDIS fill:#bfb
    style MINIO fill:#bfb
```

## Workspace Create Pipeline

```mermaid
flowchart TD
    START([tncli workspace create]) --> V

    V[1. Validate] --> P
    P[2. Provision<br/>allocate slots + create folder] --> I
    I[3. Infra<br/>shared compose + start containers + create DBs] --> S

    S --> S1[4a. Source<br/>git worktree add<br/>repo A]
    S --> S2[4b. Source<br/>git worktree add<br/>repo B]
    S --> S3[4c. Source<br/>git worktree add<br/>repo C]

    S1 --> C1[5a. Configure<br/>.env + compose override]
    S2 --> C2[5b. Configure]
    S3 --> C3[5c. Configure]

    C1 --> U1[6a. Setup<br/>tmux window<br/>npm install && migrate]
    C2 --> U2[6b. Setup]
    C3 --> U3[6c. Setup]

    U1 --> N
    U2 --> N
    U3 --> N

    N[7. Network<br/>docker network create] --> DONE([Workspace Ready])

    style V fill:#ddd
    style P fill:#fdb
    style I fill:#fdb
    style S1 fill:#bfb
    style S2 fill:#bfb
    style S3 fill:#bfb
    style C1 fill:#bbf
    style C2 fill:#bbf
    style C3 fill:#bbf
    style U1 fill:#f9f
    style U2 fill:#f9f
    style U3 fill:#f9f
    style N fill:#ddd
```

## Workspace Delete Pipeline

```mermaid
flowchart TD
    START([tncli workspace delete]) --> STOP
    STOP[1. Stop<br/>caller handles] --> REL
    REL[2. Release<br/>free shared slots] --> CLEAN
    CLEAN[3. Cleanup<br/>run pre_delete commands] --> REM
    REM[4. Remove<br/>git worktree remove<br/>drop databases<br/>release port block] --> FIN
    FIN[5. Finalize<br/>docker network rm<br/>delete folder] --> DONE([Deleted])

    style STOP fill:#ddd
    style REL fill:#fdb
    style CLEAN fill:#f9f
    style REM fill:#bbf
    style FIN fill:#ddd
```

## Migrate Pipeline

```mermaid
flowchart TD
    START([tncli migrate]) --> XDG

    XDG["1. XDG Migration<br/>~/.tncli/ -> ~/.local/state/tncli/"] --> CLEAN
    CLEAN["2. Clean Old Files<br/>Caddy, proxy, loopback script"] --> NET
    NET["3. Network State<br/>reset slots, re-init dynamic ports"] --> SLOTS
    SLOTS["4. Stale Slots<br/>remove deleted workspace slots"] --> SYS
    SYS["5. System Config (sudo)<br/>/etc/resolver, /etc/hosts old,<br/>loopback aliases 127.0.1.x"] --> HOSTS
    HOSTS["6. /etc/hosts (sudo)<br/>add shared service names"] --> REGEN
    REGEN["7. Regenerate<br/>shared compose + env files<br/>+ compose overrides"] --> RESTART
    RESTART["8. Restart<br/>docker compose down + up"] --> DONE([Migration Complete])

    style XDG fill:#fdb
    style CLEAN fill:#fdb
    style NET fill:#bbf
    style SLOTS fill:#bbf
    style SYS fill:#f99
    style HOSTS fill:#f99
    style REGEN fill:#bfb
    style RESTART fill:#bfb
```

## Template Resolution

```mermaid
flowchart LR
    subgraph INPUT["Template Input"]
        T1["{{host:postgres}}"]
        T2["{{port:postgres}}"]
        T3["{{url:minio}}"]
        T4["{{conn:postgres}}"]
        T5["{{db:0}}"]
        T6["{{slot:redis}}"]
    end

    subgraph RESOLVE["Resolution"]
        R1["service name"]
        R2["SharedPort()"]
        R3["http://host:port"]
        R4["user:pass@host:port"]
        R5["session_branchsafe"]
        R6["slot index"]
    end

    subgraph OUTPUT["Output"]
        O1["postgres"]
        O2["44800"]
        O3["http://minio:44802"]
        O4["postgres:postgres@postgres:44800"]
        O5["myproject_feat_login"]
        O6["3"]
    end

    T1 --> R1 --> O1
    T2 --> R2 --> O2
    T3 --> R3 --> O3
    T4 --> R4 --> O4
    T5 --> R5 --> O5
    T6 --> R6 --> O6
```

## Dependency Graph

```mermaid
graph LR
    subgraph "Start Order (dependencies first)"
        direction LR
        DB[db] --> WORKER[worker] --> API[api]
    end
```

```yaml
services:
  api:
    cmd: dip server
    depends_on: [worker]
  worker:
    cmd: dip sidekiq
    depends_on: [db]
  db:
    cmd: dip db
```

Kahn's toposort with cycle detection. Start: dependencies first. Stop: reverse.
Transitive: requesting `api` auto-starts `worker` and `db`.

## Interfaces

```mermaid
classDiagram
    class Runner {
        <<interface>>
        +SessionExists(session) bool
        +ListWindows(session) map
        +CreateSessionIfNeeded(session) bool
        +NewWindow(session, name, cmd)
        +GracefulStop(session, window)
        +KillSession(session)
        +CapturePane(session, window, lines) []string
    }

    class ExecRunner {
        exec.Command("tmux", ...)
    }

    class GitRunner {
        <<interface>>
        +ListWorktrees(dir) []GitWorktree
        +CurrentBranch(dir) string
        +CreateWorktreeFromBase(...) string, error
        +RemoveWorktree(...) error
    }

    class ExecGitRunner {
        exec.Command("git", ...)
    }

    class DockerRunner {
        <<interface>>
        +CreateNetwork(name) error
        +RemoveNetwork(name)
        +ForceCleanup(project)
    }

    class ExecDockerRunner {
        exec.Command("docker", ...)
    }

    Runner <|.. ExecRunner
    GitRunner <|.. ExecGitRunner
    DockerRunner <|.. ExecDockerRunner
```

## State Files

```mermaid
graph TD
    subgraph XDG["~/.local/state/tncli/"]
        SLOTS[slots.json<br/>session slot leases]
        SHARED[shared_slots.json<br/>capacity allocations]
        REG[registry.json<br/>session -> project dir]
        NODE[node-bind-host.js<br/>Node.js BIND_IP patch]
        COLLAPSE[collapse-session.json<br/>TUI collapse state]
        PIPELINE[pipeline-branch.json<br/>resume state]
        ACTIVE[active/branch<br/>active markers]
    end

    subgraph PROJECT["project/.tncli/"]
        NETWORK[network.json<br/>slot, blocks,<br/>service_map, shared_map]
    end

    style XDG fill:#eef
    style PROJECT fill:#efe
```
