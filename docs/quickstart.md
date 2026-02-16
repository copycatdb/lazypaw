# Quick Start: Dev to Prod

Go from zero to a production REST API for SQL Server in 15 minutes.

---

## Step 1: Start locally (30 seconds)

```bash
npx lazypaw --server localhost --database mydb --user sa --password YourPass123!
```

That's it. Every table and view is now a REST endpoint:

```
✨ lazypaw running at http://localhost:3000
📖 Swagger UI at http://localhost:3000/swagger
📡 4 tables, 1 view detected
```

No config files. No code generation. No ORM setup.

> **Don't have a SQL Server?** Spin one up:
> ```bash
> docker run -d -p 1433:1433 -e ACCEPT_EULA=Y -e MSSQL_SA_PASSWORD=YourPass123! \
>   mcr.microsoft.com/mssql/server:2022-latest
> ```

---

## Step 2: Query your data

### REST (curl)

```bash
# All users
curl http://localhost:3000/users

# Filter + sort + paginate
curl 'http://localhost:3000/users?status=eq.active&order=created_at.desc&limit=10'

# Join related data (FK embedding)
curl 'http://localhost:3000/users?select=name,email,orders(id,total,order_items(*))'

# Insert
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -H "Prefer: return=representation" \
  -d '{"name": "Alice", "email": "alice@example.com"}'

# Update
curl -X PATCH 'http://localhost:3000/users?id=eq.42' \
  -H "Content-Type: application/json" \
  -d '{"status": "inactive"}'

# Delete
curl -X DELETE 'http://localhost:3000/users?id=eq.42'

# Call stored procedure
curl -X POST http://localhost:3000/rpc/get_leaderboard \
  -d '{"top_n": 10}'
```

### SDK (TypeScript)

```bash
npm install lazypaw-js
```

```typescript
import { createClient } from 'lazypaw-js'

const db = createClient('http://localhost:3000')

// Read with filters + joins
const { data } = await db.from('users')
  .select('name, email, orders(id, total)')
  .eq('status', 'active')
  .order('created_at', { ascending: false })
  .limit(10)

// Insert
await db.from('users').insert({ name: 'Alice', email: 'alice@example.com' })

// Update
await db.from('users').update({ status: 'inactive' }).eq('id', 42)

// Delete
await db.from('users').delete().eq('id', 42)
```

If you've used Supabase, you already know the API. It's identical.

---

## Step 3: Add type safety

Generate TypeScript types from your database schema:

```bash
npx lazypaw codegen --server localhost --database mydb --user sa --password YourPass123! \
  --lang typescript --output ./src/db-types.ts
```

This gives you:

```typescript
// db-types.ts (auto-generated)
export interface Database {
  users: {
    Row: { id: number; name: string; email: string; status: string; created_at: string }
    Insert: { name: string; email: string; status?: string }
    Update: { name?: string; email?: string; status?: string }
  }
  orders: {
    Row: { id: number; user_id: number; total: number; created_at: string }
    Insert: { user_id: number; total: number }
    Update: { total?: number }
  }
}
```

Use it:

```typescript
import type { Database } from './db-types'
const db = createClient<Database>('http://localhost:3000')

const { data } = await db.from('users').select('*')
//    ^? Database['users']['Row'][]  ← full autocomplete
```

---

## Step 4: Add authentication

lazypaw works with **any** OIDC provider — Auth0, Entra ID, Firebase, Keycloak, etc.

### 1. Configure lazypaw

Create `lazypaw.toml`:

```toml
server = "localhost"
database = "mydb"
user = "lazypaw_service"
listen_port = 3000

[auth]
mode = "oidc"
issuer = "https://your-tenant.auth0.com/"
audience = "api://lazypaw"
anon_role = "anon"                         # unauthenticated requests use this DB role
role_claim = "app_role"                     # JWT claim → DB role mapping
context_claims = ["sub", "email", "tenant_id"]

[auth.role_map]
"admin" = "app_admin"
"user" = "app_user"
```

### 2. Set up database roles

```sql
-- Service account (lazypaw connects as this)
CREATE LOGIN lazypaw_service WITH PASSWORD = 'StrongPass!';
CREATE USER lazypaw_service FOR LOGIN lazypaw_service;
GRANT IMPERSONATE ON USER::app_admin TO lazypaw_service;
GRANT IMPERSONATE ON USER::app_user TO lazypaw_service;
GRANT IMPERSONATE ON USER::anon TO lazypaw_service;

-- App roles
CREATE USER app_admin WITHOUT LOGIN;
CREATE USER app_user WITHOUT LOGIN;
CREATE USER anon WITHOUT LOGIN;

GRANT SELECT, INSERT, UPDATE, DELETE ON SCHEMA::dbo TO app_admin;
GRANT SELECT, INSERT ON SCHEMA::dbo TO app_user;
GRANT SELECT ON dbo.public_content TO anon;
```

### 3. Add Row-Level Security

```sql
CREATE FUNCTION dbo.rls_users(@user_id NVARCHAR(128))
RETURNS TABLE AS RETURN
  SELECT 1 AS ok WHERE @user_id = CONVERT(NVARCHAR(128), SESSION_CONTEXT(N'sub'));

CREATE SECURITY POLICY dbo.users_policy
  ADD FILTER PREDICATE dbo.rls_users(id) ON dbo.users;
```

Now each request runs as the mapped DB role with JWT claims in session context. Users only see their own data.

### 4. SDK with auth

```typescript
const db = createClient('http://localhost:3000', {
  tokenFn: async () => await getAccessToken()  // your auth library
})
```

---

## Step 5: Add realtime

Enable live change notifications via WebSocket:

```bash
lazypaw --config lazypaw.toml --realtime
```

```typescript
db.channel('orders')
  .on('INSERT', (payload) => {
    console.log('New order:', payload.record)
    // Update UI, send notification, etc.
  })
  .subscribe()
```

Uses SQL Server Change Tracking under the hood — works on all Azure SQL tiers.

---

## Step 6: Deploy to production

### Option A: Docker (recommended)

```yaml
# docker-compose.yml
services:
  lazypaw:
    image: ghcr.io/copycatdb/lazypaw:latest
    ports:
      - "3000:3000"
    environment:
      LAZYPAW_SERVER: your-db.database.windows.net
      LAZYPAW_DATABASE: prod
      LAZYPAW_USER: lazypaw_service
      LAZYPAW_PASSWORD_FILE: /run/secrets/db_password
      LAZYPAW_AUTH_MODE: oidc
      LAZYPAW_OIDC_ISSUER: https://your-tenant.auth0.com/
      LAZYPAW_OIDC_AUDIENCE: api://lazypaw
      LAZYPAW_ANON_ROLE: anon
      LAZYPAW_LOG_FORMAT: json
      LAZYPAW_LOG_LEVEL: info
      LAZYPAW_REALTIME: "true"
    secrets:
      - db_password

secrets:
  db_password:
    file: ./secrets/db_password.txt
```

### Option B: Azure Container Apps

```bash
az containerapp create \
  --name lazypaw \
  --image ghcr.io/copycatdb/lazypaw:latest \
  --target-port 3000 \
  --env-vars \
    LAZYPAW_SERVER=your-db.database.windows.net \
    LAZYPAW_DATABASE=prod \
    LAZYPAW_USER=lazypaw_service \
    LAZYPAW_PASSWORD=secretref:db-password \
    LAZYPAW_AUTH_MODE=oidc \
    LAZYPAW_OIDC_ISSUER=https://your-tenant.auth0.com/
```

### Option C: Fly.io

```toml
# fly.toml
[http_service]
  internal_port = 3000

[env]
  LAZYPAW_SERVER = "your-db.database.windows.net"
  LAZYPAW_DATABASE = "prod"
  LAZYPAW_AUTH_MODE = "oidc"
  LAZYPAW_LOG_FORMAT = "json"
  LAZYPAW_REALTIME = "true"
```

```bash
fly secrets set LAZYPAW_PASSWORD=your-db-password
fly deploy
```

---

## Production checklist

### Security
- [ ] **Never use SA** — create a dedicated service account with minimal permissions
- [ ] **Enable auth** — `auth.mode = "oidc"` with your identity provider
- [ ] **Add RLS** — row-level security so users only see their data
- [ ] **TLS** — terminate TLS at reverse proxy (nginx/Caddy) or use Azure Front Door
- [ ] **Network isolation** — lazypaw should only be reachable via your proxy, not directly

### Performance
- [ ] **Pool size** — default is 10, increase for high concurrency: `--pool-size 50`
- [ ] **Logging** — `--log-format json` for production log aggregation
- [ ] **Slow queries** — `--log-slow-queries 500` to flag queries over 500ms
- [ ] **Connection** — use a private endpoint or VNet for DB connectivity

### Monitoring
- [ ] **Health check** — `GET /` returns 200 when healthy
- [ ] **Structured logs** — JSON format pipes into Datadog/Splunk/ELK
- [ ] **OpenTelemetry** — build with `--features otel` and set `--otel-endpoint` for distributed tracing

### Schema management
- [ ] **Codegen in CI** — regenerate types on schema changes: `lazypaw codegen --lang typescript`
- [ ] **Migrations** — use your existing migration tool (Flyway, dbmate, etc.) — lazypaw just reads the schema

---

## Architecture overview

```
Client (browser/mobile/server)
  │
  ├── SDK (lazypaw-js)  ──── HTTPS ────┐
  │                                      │
  └── WebSocket  ────── WSS ────────────┤
                                         │
                                    [ lazypaw ]
                                    │    │    │
                              CRUD  │  Auth   │  Realtime
                              REST  │  OIDC   │  Change Tracking
                                    │  JWKS   │
                                    │  RLS    │
                                         │
                                    SQL Server
```

lazypaw is a single binary (~15MB). No runtime, no dependencies, no sidecar services. It validates JWTs, maps claims to database roles, and translates REST requests into SQL — that's it.

---

## Next steps

| Want to... | Read... |
|---|---|
| See every filter and operator | [API Reference](./api-reference.md) |
| Configure Auth0/Entra/Firebase | [Auth Guide](./auth-guide.md) |
| Set up row-level security | [RLS Guide](./rls-guide.md) |
| Use the full SDK API | [SDK Reference](./sdk-reference.md) |
| Generate types for Python | [Codegen](./codegen.md) |
| Compare with DAB | [lazypaw vs DAB](./comparison.md) |
