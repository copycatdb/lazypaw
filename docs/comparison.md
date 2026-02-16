# lazypaw vs Data API builder (DAB)

Both lazypaw and [Data API builder](https://github.com/Azure/data-api-builder) turn a SQL Server database into a REST API without writing backend code. That's where the similarities end.

## At a glance

| | **lazypaw** | **DAB** |
|---|---|---|
| Maker | CopyCat (indie/open-source) | Microsoft (Azure team) |
| Language | Rust | C# (.NET 8) |
| Codebase | ~4K lines | ~6.8M lines |
| Binary | Single static executable | Requires .NET runtime |
| Startup | Milliseconds | Seconds (JIT warmup) |
| Databases | SQL Server | SQL Server, PostgreSQL, MySQL, Cosmos DB |
| API style | REST (PostgREST-compatible) | REST + GraphQL |
| Query syntax | PostgREST / Supabase standard | OData-inspired ($filter, $orderby) |
| Config | Zero-config (auto-introspects schema) | JSON config per entity |
| Client SDK | lazypaw-js (Supabase-compatible) | None |
| Realtime | WebSocket (Change Tracking) | None |
| Type codegen | `lazypaw codegen --lang ts\|python` | None |
| Auth | Any OIDC provider → DB impersonation (EXECUTE AS USER) | EasyAuth / JWT → config-file permissions |
| RLS | Native SQL Server RLS via SESSION_CONTEXT | Permission rules in JSON config |
| License | MIT | MIT |

## Setup

### lazypaw

```bash
lazypaw --server localhost --database mydb --user sa --password pass
# Done. Every table and view is an endpoint.
```

### DAB

```bash
dotnet tool install microsoft.dataapibuilder -g
dab init --database-type mssql --connection-string "@env('conn')" --host-mode development
dab add Todo --source dbo.Todo --permissions "anonymous:*"
dab add Users --source dbo.Users --permissions "anonymous:read" --permissions "authenticated:*"
dab add Orders --source dbo.Orders --permissions "authenticated:*" --rest.path "/orders"
# Repeat for every table...
dab start
```

lazypaw introspects `INFORMATION_SCHEMA` and `sys.foreign_keys` at startup — every table becomes an endpoint automatically. DAB requires you to register each entity in a JSON config file and define its permissions, REST path, and GraphQL type individually.

## Query syntax

### Filtering

```bash
# lazypaw (PostgREST / Supabase standard)
GET /users?status=eq.active&age=gte.21&name=like.%smith%

# DAB (OData-inspired)
GET /api/Users?$filter=status eq 'active' and age ge 21 and contains(name, 'smith')
```

### Ordering & pagination

```bash
# lazypaw
GET /users?order=created_at.desc&limit=10&offset=20

# DAB
GET /api/Users?$orderby=created_at desc&$first=10&$after=<cursor>
```

### FK embedding (joins)

```bash
# lazypaw — nested resources in one request
GET /users?select=name,email,orders(id,total,items(product,qty))

# DAB — no nested embedding in REST
# Must make separate requests or use GraphQL:
# query { users { name email orders { id total items { product qty } } } }
```

lazypaw's FK embedding works in REST with PostgREST syntax. DAB only supports nested relationships through GraphQL.

### Column selection

```bash
# lazypaw
GET /users?select=id,name,email

# DAB
GET /api/Users?$select=id,name,email
```

## Client SDK

### lazypaw (Supabase-compatible)

```typescript
import { createClient } from 'lazypaw-js'

const lp = createClient('http://localhost:3000')

// Fluent, typed, familiar
const { data } = await lp.from('users')
  .select('*, orders(total)')
  .eq('status', 'active')
  .order('created_at', { ascending: false })
  .limit(10)

// Insert / upsert / update / delete
await lp.from('users').insert({ name: 'Alice', email: 'alice@co.com' })
await lp.from('users').upsert({ id: 1, score: 100 })
await lp.from('users').update({ status: 'inactive' }).eq('id', 1)
await lp.from('users').delete().eq('id', 1)

// Stored procedures
const { data: report } = await lp.rpc('monthly_report', { month: 6 })

// Realtime
lp.channel('orders')
  .on('INSERT', (payload) => console.log('New order:', payload.record))
  .subscribe()
```

### DAB

```typescript
// No SDK. Raw fetch:
const res = await fetch(
  `/api/Users?$filter=status eq 'active'&$orderby=created_at desc&$first=10`,
  { headers: { 'Authorization': `Bearer ${token}` } }
)
const data = await res.json()

// Insert
await fetch('/api/Users', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ name: 'Alice', email: 'alice@co.com' })
})

// No realtime. Poll or add SignalR yourself.
```

## Realtime

| | **lazypaw** | **DAB** |
|---|---|---|
| Transport | WebSocket (`/realtime`) | None |
| Events | INSERT, UPDATE, DELETE | — |
| Mechanism | SQL Server Change Tracking poll (200ms) | — |
| Latency | ~100-300ms | — |
| Filtered subscriptions | Yes | — |
| Fallback needed | No | SignalR, polling, or external service |

lazypaw includes realtime out of the box. Subscribe to table changes over WebSocket with optional filters. DAB has no realtime capability — you need to build your own notification layer with SignalR, Azure Event Grid, or polling.

## Type codegen

### lazypaw

```bash
lazypaw codegen --lang typescript --output ./src/db-types.ts
lazypaw codegen --lang python --output ./db_types.py
```

Generates `Row`, `Insert`, `Update` types per table. Knows about IDENTITY columns (skipped from Insert), DEFAULT constraints (optional in Insert), computed columns (excluded). Plugs into the SDK for full type safety:

```typescript
import type { Database } from './db-types'
const lp = createClient<Database>('http://localhost:3000')

const { data } = await lp.from('users').select('*')
//    ^? Database['users']['Row'][]
```

### DAB

No type generation. You write types manually or use a third-party tool.

## Auth & permissions

### lazypaw

Auth lives in the database. lazypaw is a bridge:

```
OIDC Provider (Auth0, Entra, Firebase, etc.)
    → JWT with claims { sub, role, tenant_id }
    → lazypaw validates via JWKS discovery
    → Maps claims to DB role (config: role_map)
    → EXECUTE AS USER = 'mapped_role'
    → sp_set_session_context('user_id', sub)
    → SQL Server RLS policies filter rows
    → REVERT
```

Permissions are SQL Server RLS policies — standard, portable, auditable:

```sql
CREATE SECURITY POLICY tenant_filter
ADD FILTER PREDICATE dbo.fn_tenant_check(tenant_id) ON dbo.orders;
```

### DAB

Auth lives in the config file:

```json
{
  "entities": {
    "Users": {
      "permissions": [
        { "role": "anonymous", "actions": ["read"] },
        { "role": "authenticated", "actions": ["read", "update"] },
        { "role": "admin", "actions": ["*"] }
      ]
    }
  }
}
```

DAB enforces permissions in its C# middleware, not in the database. This means:
- Permissions aren't visible in SQL Server
- Can't reuse DB-level RLS policies
- Config changes require restart
- Permission logic is split across config + code, not centralized in SQL

## Stored procedures

### lazypaw

```bash
POST /rpc/get_leaderboard
Content-Type: application/json

{"game_id": 1, "top_n": 10}
```

→ `EXEC get_leaderboard @game_id = 1, @top_n = 10`

### DAB

```bash
POST /api/GetLeaderboard
Content-Type: application/json

{"game_id": 1, "top_n": 10}
```

Similar, but requires registering the procedure in config first:

```json
{
  "entities": {
    "GetLeaderboard": {
      "source": { "type": "stored-procedure", "object": "dbo.get_leaderboard" },
      "permissions": [{ "role": "anonymous", "actions": [{ "action": "execute" }] }],
      "rest": { "methods": ["POST"] }
    }
  }
}
```

## Deployment

### lazypaw

```bash
# Single binary, no dependencies
curl -L https://github.com/copycatdb/lazypaw/releases/download/v1.0/lazypaw-linux-x64 -o lazypaw
chmod +x lazypaw
./lazypaw --server db.example.com --database prod

# Or Docker
docker run -p 3000:3000 copycatdb/lazypaw --server db --database prod
```

8MB binary. No runtime. Starts in milliseconds.

### DAB

```bash
# Requires .NET 8 runtime
dotnet tool install microsoft.dataapibuilder -g
dab start --config dab-config.json

# Or Docker (includes .NET runtime)
docker run -p 5000:5000 -v ./config:/app/config mcr.microsoft.com/azure-databases/data-api-builder
```

Needs .NET runtime or a ~200MB container image.

## What DAB has that lazypaw doesn't

- **GraphQL endpoint** — full schema-based GraphQL API alongside REST
- **Multi-database** — PostgreSQL, MySQL, Cosmos DB support (lazypaw is SQL Server only)
- **Azure integration** — Azure Static Web Apps, Azure SQL, Cosmos DB managed identity
- **MCP support** — announced, coming soon
- **Microsoft backing** — official docs on learn.microsoft.com, enterprise support path
- **4+ years of production hardening** — 61 releases

## What lazypaw has that DAB doesn't

- **Zero-config** — auto-introspects schema, no JSON entity registration
- **Supabase-compatible** — PostgREST query syntax, familiar to millions of developers
- **Client SDK** — lazypaw-js with fluent query builder
- **Realtime** — WebSocket change notifications, built in
- **Type codegen** — generate TypeScript/Python types from your schema
- **FK embedding in REST** — nested joins without GraphQL
- **DB-native auth** — EXECUTE AS USER + RLS, not config-file permissions
- **Single static binary** — no runtime dependencies, 8MB
- **Rust performance** — no GC pauses, no JIT warmup

## Who should use what

**Use DAB if:**
- You need GraphQL
- You need multi-database support (Postgres, MySQL, Cosmos)
- You're deploying on Azure Static Web Apps
- You want Microsoft-backed enterprise support
- Your team prefers explicit config-file control over every entity

**Use lazypaw if:**
- You want the fastest path from SQL Server to API
- Your frontend team knows Supabase/PostgREST
- You need realtime (WebSocket push)
- You want typed client SDKs
- You want DB-native security (RLS, EXECUTE AS USER)
- You value small, fast, dependency-free binaries
- You want to ship today, not next sprint
