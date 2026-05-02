# Port Allocation Design

Dynamic port allocation for both workspace services and shared services.

## Port Pool Layout

```
Global pool: 40000–49999 (10,000 ports)

Slot 0 (session A): 40000–44999  (5,000 ports)
Slot 1 (session B): 45000–49999  (5,000 ports)

Within each slot:
┌──────────────────────────────────────────────────────────────┐
│ Workspace blocks (from bottom)    │ Shared services (from top)│
│ 40000 ─────────────── 44799       │ 44800 ─────────── 44999  │
│ 48 blocks × 100 ports = 4,800    │ 200 ports reserved       │
└──────────────────────────────────────────────────────────────┘
```

## Constants

```
PoolStart     = 40000
PoolEnd       = 49999
SlotSize      = 5000      # ports per session slot
BlockSize     = 100       # ports per workspace block
SharedReserve = 200       # ports reserved at top for shared services
MaxSlots      = 2         # max concurrent sessions
MaxBlocks     = (SlotSize - SharedReserve) / BlockSize  # = 48
```

## Formulas

### Session Slot

```
slotBase(slot) = PoolStart + slot × SlotSize
slotTop(slot)  = slotBase(slot) + SlotSize - 1
```

Slot 0: 40000–44999
Slot 1: 45000–49999

### Workspace Service Port

Each workspace claims a block (0–47). Each service gets a stable index within the block.

```
port(slot, blockIdx, svcIdx) = slotBase(slot) + blockIdx × BlockSize + svcIdx
```

Example (slot 0, block 3, service 2):
```
40000 + 3 × 100 + 2 = 40302
```

### Shared Service Port

Shared services occupy the top section of each slot, counting UP from the shared base.

```
sharedBase(slot) = slotBase(slot) + SlotSize - SharedReserve
sharedPort(slot, offset) = sharedBase(slot) + offset
```

Example (slot 0):
```
sharedBase = 40000 + 5000 - 200 = 44800
postgres (offset 0): 44800
redis    (offset 1): 44801
minio    (offset 2): 44802  (port 9000)
minio    (offset 3): 44803  (port 9090, console)
```

### Multi-Port Services

Services with multiple port mappings (e.g., minio has 9000 + 9090) get consecutive offsets:

```
Port mapping 0 → sharedPort(slot, baseOffset)
Port mapping 1 → sharedPort(slot, baseOffset + 1)
...
```

### Multi-Instance Services (Capacity)

Services with `capacity` (e.g., redis with 16 DB slots) can auto-scale to multiple instances.
Each instance offsets from the service's base port:

```
instancePort(slot, baseOffset, instanceIdx) = sharedPort(slot, baseOffset) + instanceIdx
```

Reserved offset space per service = `maxInstances × numPorts`

Example (redis, 3 instances, 1 port each):
```
redis offset 1:
  instance 0: 44801
  instance 1: 44802
  instance 2: 44803
Next service starts at offset 1 + 3 = 4
```

## SharedMap Storage

`network.json` stores:

```json
{
  "slot": 0,
  "blocks": { "ws-main": 0, "ws-feat-login": 1 },
  "service_map": { "api~server": 0, "api~worker": 1, "client~dev": 2 },
  "shared_map": { "postgres": 0, "redis": 1, "minio": 4, "opensearch": 6 }
}
```

`shared_map` values are base offsets. The code knows how many ports each service needs
from `len(ports)` in config and reserves contiguous offsets.

## tncli.yml — Shared Service Ports

Ports in config specify container ports only. Host ports are dynamically allocated.

```yaml
shared_services:
  postgres:
    image: postgres:16
    ports: ["5432"]              # container port only
  redis:
    image: redis:7-alpine
    ports: ["6379"]
    capacity: 16
  minio:
    image: minio/minio
    ports: ["9000", "9090"]      # 2 container ports → 2 dynamic host ports
  opensearch:
    image: opensearchproject/opensearch:2.11.0
    ports: ["9200", "9600"]
```

## Template Resolution

`{{port:NAME}}` resolves to `SharedPort(projectDir, name)` for shared services,
or `Port(projectDir, wsKey, svcKey)` for workspace services.

```
{{port:postgres}}  → 44800  (slot 0)
{{port:redis}}     → 44801
{{port:minio}}     → 44804
{{port:opensearch}}→ 44806
```

## Docker Compose Generation

`GenerateSharedCompose()` maps:

```yaml
services:
  postgres:
    ports:
      - "44800:5432"    # SharedPort(dir, "postgres"):containerPort
  redis:
    ports:
      - "44801:6379"    # instance 0
  redis-2:
    ports:
      - "44802:6379"    # instance 1 = basePort + 1
  minio:
    ports:
      - "44804:9000"    # port mapping 0
      - "44805:9090"    # port mapping 1
```

## Collision Safety

```
Workspace blocks: 40000–44799  (48 blocks × 100 ports)
Shared services:  44800–44999  (200 ports)
                  ↑ no overlap ↑
```

Max 200 shared service port mappings × instances. More than enough for typical setups.

## Docker Networking

Shared services and workspace containers join `tncli-shared` Docker network.

Template `{{host:NAME}}` resolves to the service name (e.g. `postgres`, `minio`).
This name is resolved via:
- **Host/browser**: `/etc/hosts` → `127.0.0.1` (managed by `tncli setup`)
- **Docker container**: `extra_hosts: NAME:host-gateway` → host IP

Same URL works everywhere: `http://minio:44804` resolves correctly from browser,
host processes, and Docker containers (macOS + Linux).
