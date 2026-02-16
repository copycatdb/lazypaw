# lazypaw 😴

**Instant REST API for SQL Server. No code. No ORM. No backend.**

Part of [CopyCat](https://github.com/copycatdb) 🐱

---

> PostgreSQL has [PostgREST](https://github.com/PostgREST/postgrest) (26K+ stars, 43M+ Docker pulls), [Supabase](https://supabase.com), and an entire ecosystem of instant API tools.
>
> **SQL Server has nothing.**
>
> Until now.

---

## What is this?

Point lazypaw at any SQL Server database and get a full REST API — with filtering, pagination, relationships, auth, realtime, and a typed client SDK. Zero backend code.

```bash
# That's it. That's the whole backend.
lazypaw --server localhost --database mydb --user sa --password pass
```

Your tables are now API endpoints:

```bash
GET    /users                          # list
GET    /users?id=eq.42                 # filter
GET    /users?select=name,email        # column selection
GET    /users?select=*,orders(*)       # FK embedding (joins!)
GET    /users?limit=10&offset=20       # pagination
GET    /users?order=created_at.desc    # ordering
POST   /users                          # insert
PATCH  /users?id=eq.42                 # update
DELETE /users?id=eq.42                 # delete
POST   /rpc/get_leaderboard            # stored procedures
```

## Why?

Because building a CRUD API shouldn't require 47 Express routes, a Prisma schema, 12 DTOs, and a controller layer just to SELECT from a table.

Every enterprise runs SQL Server. None of them have an instant API layer. lazypaw fixes that.

**lazypaw is for:**
- 🏢 Internal tools and admin dashboards
- 📱 Mobile & SPA backends — skip the middleware
- ⚡ Prototypes and hackathons — API in 30 seconds
- 🔌 AI/LLM tool access — give agents structured DB access
- 🛠️ Migrations — drop-in Supabase-style DX on existing SQL Server databases
- 😴 People who value their weekends

## Features

### API (PostgREST-compatible)
- **CRUD** — GET, POST, PATCH, DELETE with `OUTPUT inserted.*`
- **Filtering** — eq, neq, gt, gte, lt, lte, like, ilike, is, in, not, fts
- **FK Embedding** — `?select=*,orders(items(*))` resolves foreign keys as nested JSON
- **Pagination** — limit, offset, ordering, `Content-Range` headers
- **Upsert** — `Prefer: resolution=merge-duplicates` → T-SQL `MERGE`
- **RPC** — `POST /rpc/proc_name` → `EXEC stored_procedure`
- **Content negotiation** — JSON, CSV (`text/csv`), Arrow IPC (`application/vnd.apache.arrow.stream`)
- **OpenAPI** — auto-generated spec at `/`, Swagger UI at `/swagger`

### Auth (provider-agnostic)
- **Any OAuth/OIDC provider** — Auth0, Entra ID, Firebase, Keycloak, Okta, Supabase Auth
- **JWT validation** — RS256/384/512 via JWKS discovery, HS256 fallback
- **Claim → DB role mapping** — `role_map: { "app_admin": "db_admin" }`
- **Per-request impersonation** — `EXECUTE AS USER` + `REVERT`
- **Session context** — JWT claims injected via `sp_set_session_context` for RLS policies
- **Azure Managed Identity** — IMDS + Workload Identity for passwordless DB auth
- **`lazypaw setup`** — generates SQL scripts for DBA review, no magic

### Realtime (WebSocket)
- **Change notifications** — INSERT, UPDATE, DELETE pushed via WebSocket
- **Change Tracking** — lightweight, all Azure SQL tiers, ~100-300ms latency
- **Filtered subscriptions** — subscribe to specific tables + filter conditions
- **Built-in** — no separate server, no Kafka, no Redis

### Client SDK (`lazypaw-js`)
Supabase-js compatible API. If you know Supabase, you know lazypaw.

```typescript
import { createClient } from 'lazypaw-js'

const lp = createClient('http://localhost:3000')

// Read
const { data } = await lp.from('games')
  .select('*, players(name, score)')
  .eq('status', 'active')
  .order('created_at', { ascending: false })
  .limit(10)

// Write
await lp.from('players').insert({ name: 'Alice', score: 0 })
await lp.from('players').upsert({ id: 1, score: 100 })
await lp.from('players').update({ score: 200 }).eq('id', 1)
await lp.from('players').delete().eq('id', 1)

// Stored procedures
const { data: leaders } = await lp.rpc('get_leaderboard', { game_id: 1 })

// Realtime — live updates via WebSocket
lp.channel('players')
  .on('INSERT', (payload) => console.log('New player:', payload.record))
  .on('UPDATE', (payload) => console.log('Score changed:', payload.record))
  .subscribe()
```

### Codegen (typed clients)
Generate fully-typed client code from your database schema:

```bash
lazypaw codegen --lang typescript --output ./src/db-types.ts
lazypaw codegen --lang python --output ./db_types.py
```

```typescript
// AUTO-GENERATED — Row, Insert, Update types per table
import type { Database } from './db-types'

const lp = createClient<Database>('http://localhost:3000')

const { data } = await lp.from('players').select('*')
//    ^? Database['players']['Row'][]     ← full type safety
```

## Architecture

```
Client → lazypaw (Rust/axum) → claw → tabby → SQL Server
           |
           ├── Schema introspection (startup + SIGHUP reload)
           ├── Route generation (automatic from tables/views)
           ├── Query building (URL params → T-SQL)
           ├── Auth (OIDC/JWKS → EXECUTE AS USER)
           ├── Realtime (Change Tracking → WebSocket fan-out)
           └── Response serialization (JSON/CSV/Arrow)
```

Built on the [CopyCat](https://github.com/copycatdb) stack:
- **[tabby](https://github.com/copycatdb/tabby)** — Rust TDS 7.4+ wire protocol (no ODBC, no drivers)
- **[claw](https://github.com/copycatdb/claw)** — High-level SQL Server client + Arrow support

Single static binary. No runtime dependencies. No JVM. No Node. Just the executable and your database.

## Quick Start

```bash
# 1. Start lazypaw
lazypaw --server myserver.database.windows.net \
        --database mydb \
        --user api_service \
        --password $DB_PASSWORD

# 2. Generate types (optional)
lazypaw codegen --lang typescript --output ./src/db.ts

# 3. Query from anywhere
curl 'http://localhost:3000/users?select=name,email&status=eq.active&limit=10'
```

## Supabase Parity

If you're coming from Supabase/PostgREST, here's what maps 1:1:

| Supabase | lazypaw | T-SQL |
|----------|---------|-------|
| `.select('*')` | `.select('*')` | `SELECT *` |
| `.eq('id', 1)` | `.eq('id', 1)` | `WHERE id = 1` |
| `.order('name')` | `.order('name')` | `ORDER BY name` |
| `.limit(10).offset(20)` | `.limit(10).offset(20)` | `OFFSET 20 ROWS FETCH NEXT 10 ROWS ONLY` |
| `.range(0, 9)` | `.range(0, 9)` | `OFFSET 0 ROWS FETCH NEXT 10 ROWS ONLY` |
| `.insert({...})` | `.insert({...})` | `INSERT ... OUTPUT inserted.*` |
| `.upsert({...})` | `.upsert({...})` | `MERGE ... OUTPUT inserted.*` |
| `.update({...})` | `.update({...})` | `UPDATE ... OUTPUT inserted.*` |
| `.delete()` | `.delete()` | `DELETE ... OUTPUT deleted.*` |
| `.single()` | `.single()` | + `Accept: vnd.pgrst.object+json` |
| `.rpc('fn', args)` | `.rpc('fn', args)` | `EXEC fn @param=value` |
| `supabase.channel()` | `lp.channel()` | Change Tracking poll |
| `supabase gen types` | `lazypaw codegen` | `sys.columns` introspection |

**The API is the same. The database is different. Your code doesn't change.**

## Demo: CopyCat Trivia

The repo includes a multiplayer trivia game built entirely with lazypaw — zero backend code:

```
demo/          — React + Vite + Tailwind
sdk/           — lazypaw-js client SDK
```

Features exercised: CRUD, FK embedding, filtering, pagination, realtime WebSocket subscriptions, live scoreboard updates. See [demo/](./demo/) for source.

## Configuration

```bash
# CLI args
lazypaw --server host --database db --user u --password p --port 3000

# Or environment variables
LAZYPAW_SERVER=host LAZYPAW_DATABASE=db lazypaw

# Or TOML config
lazypaw --config lazypaw.toml
```

## SQL Server Setup

```bash
# Generate the setup script (review before running!)
lazypaw setup --server host --database db --anon-role anon --api-role api_service
```

This generates SQL for: role creation, `GRANT IMPERSONATE`, RLS policy templates, Change Tracking enablement. You review and run it — lazypaw never modifies your schema directly.

## Attribution

Built on the shoulders of [PostgREST](https://github.com/PostgREST/postgrest). They had the idea. We brought it to SQL Server. 🐱

## License

MIT
