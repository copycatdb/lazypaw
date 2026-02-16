# Getting Started with lazypaw

## What is lazypaw?

lazypaw is an instant REST API for SQL Server. Point it at any SQL Server database and get a full REST API with filtering, pagination, FK embedding (joins), authentication, realtime WebSocket subscriptions, and a typed client SDK — zero backend code required. Think PostgREST/Supabase, but for SQL Server.

## Quick Start

### Option 1: npx (fastest)

```bash
npx lazypaw --server localhost --database mydb --user sa --password yourpass
```

### Option 2: Docker Compose

```yaml
# docker-compose.yml
services:
  lazypaw:
    image: copycatdb/lazypaw
    ports:
      - "3000:3000"
    environment:
      LAZYPAW_SERVER: host.docker.internal
      LAZYPAW_DATABASE: mydb
      LAZYPAW_USER: sa
      LAZYPAW_PASSWORD: yourpass
```

```bash
docker compose up
```

### Option 3: Binary download

```bash
# Linux
curl -L https://github.com/copycatdb/lazypaw/releases/latest/download/lazypaw-linux-x64 -o lazypaw
chmod +x lazypaw

# macOS
curl -L https://github.com/copycatdb/lazypaw/releases/latest/download/lazypaw-darwin-arm64 -o lazypaw
chmod +x lazypaw

# Run
./lazypaw --server localhost --database mydb --user sa --password yourpass
```

Your API is now live at `http://localhost:3000`. Every table and view is an endpoint.

## Connect to Your Database

### CLI args

```bash
lazypaw --server myserver.database.windows.net \
        --database mydb \
        --user api_service \
        --password $DB_PASSWORD \
        --listen-port 3000 \
        --schema dbo \
        --pool-size 10
```

### Environment variables

Every CLI arg has an env var equivalent:

```bash
export LAZYPAW_SERVER=myserver.database.windows.net
export LAZYPAW_DATABASE=mydb
export LAZYPAW_USER=api_service
export LAZYPAW_PASSWORD=strongpassword
export LAZYPAW_LISTEN_PORT=3000
export LAZYPAW_SCHEMA=dbo
export LAZYPAW_POOL_SIZE=10

lazypaw
```

You can also use `LAZYPAW_PASSWORD_FILE` to read the password from a file (useful for Docker secrets).

### TOML config file

```toml
# lazypaw.toml
server = "myserver.database.windows.net"
database = "mydb"
user = "api_service"
password = "strongpassword"
listen_port = 3000
schema = "dbo"
pool_size = 10
trust_cert = false

[auth]
mode = "oidc"
issuer = "https://your-provider.auth0.com/"
audience = "api://lazypaw"
role_claim = "role"
context_claims = ["sub", "email", "custom:tenant_id"]

[auth.role_map]
"app_admin" = "app_admin"
"app_user" = "app_user"
```

```bash
lazypaw --config lazypaw.toml
```

Priority: CLI args > environment variables > TOML config file.

## Your First Query

```bash
# List all users
curl http://localhost:3000/users

# Filter
curl 'http://localhost:3000/users?status=eq.active&age=gte.21'

# Select specific columns + FK embedding
curl 'http://localhost:3000/users?select=name,email,orders(id,total)'

# Paginate and sort
curl 'http://localhost:3000/users?order=created_at.desc&limit=10&offset=20'

# Insert
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -H "Prefer: return=representation" \
  -d '{"name": "Alice", "email": "alice@example.com"}'

# Update
curl -X PATCH 'http://localhost:3000/users?id=eq.42' \
  -H "Content-Type: application/json" \
  -H "Prefer: return=representation" \
  -d '{"status": "inactive"}'

# Delete
curl -X DELETE 'http://localhost:3000/users?id=eq.42' \
  -H "Prefer: return=representation"

# Call a stored procedure
curl -X POST http://localhost:3000/rpc/get_leaderboard \
  -H "Content-Type: application/json" \
  -d '{"game_id": 1, "top_n": 10}'
```

## Add the Client SDK

```bash
npm install lazypaw-js
```

```typescript
import { createClient } from 'lazypaw-js'

const lp = createClient('http://localhost:3000')

// Read
const { data, error } = await lp.from('users')
  .select('name, email, orders(id, total)')
  .eq('status', 'active')
  .order('created_at', { ascending: false })
  .limit(10)

// Insert
await lp.from('users').insert({ name: 'Alice', email: 'alice@example.com' })

// Update
await lp.from('users').update({ status: 'inactive' }).eq('id', 42)

// Delete
await lp.from('users').delete().eq('id', 42)

// Stored procedure
const { data: leaders } = await lp.rpc('get_leaderboard', { game_id: 1 })
```

## Enable Realtime

Start lazypaw with the `--realtime` flag:

```bash
lazypaw --server localhost --database mydb --user sa --password pass --realtime
```

This enables WebSocket change notifications via SQL Server Change Tracking.

```typescript
// Subscribe to live changes
lp.channel('orders')
  .on('INSERT', (payload) => console.log('New order:', payload.record))
  .on('UPDATE', (payload) => console.log('Updated:', payload.record))
  .on('DELETE', (payload) => console.log('Deleted:', payload.record))
  .subscribe()
```

## Generate Types

```bash
# TypeScript
lazypaw codegen --lang typescript --output ./src/db-types.ts

# Python
lazypaw codegen --lang python --output ./db_types.py
```

Then use with the SDK for full type safety:

```typescript
import type { Database } from './db-types'
import { createClient } from 'lazypaw-js'

const lp = createClient<Database>('http://localhost:3000')

const { data } = await lp.from('users').select('*')
//    ^? Database['users']['Row'][]
```

## Add Authentication

lazypaw supports any OIDC provider (Auth0, Entra ID, Firebase, Keycloak, etc.). Configure it via CLI args or TOML:

```bash
lazypaw --auth-mode oidc \
        --oidc-issuer "https://your-provider.auth0.com/" \
        --oidc-audience "api://lazypaw" \
        --anon-role anon
```

Authenticated requests use `EXECUTE AS USER` to impersonate mapped database roles, and JWT claims are injected via `sp_set_session_context` for Row-Level Security.

See the [Auth Guide](./auth-guide.md) and [RLS Guide](./rls-guide.md) for full details.

## Next Steps

- [API Reference](./api-reference.md) — complete endpoint documentation
- [Auth Guide](./auth-guide.md) — JWT, OIDC, provider configs
- [RLS Guide](./rls-guide.md) — Row-Level Security patterns
- [SDK Reference](./sdk-reference.md) — lazypaw-js client API
- [Codegen](./codegen.md) — type generation for TypeScript and Python
- [Deployment](./deployment.md) — Docker, cloud, bare metal
- [lazypaw vs DAB](./comparison.md) — comparison with Data API builder
