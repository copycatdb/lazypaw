# Row-Level Security (RLS) with lazypaw

This guide walks through setting up SQL Server Row-Level Security with lazypaw so that each API request automatically sees only the rows it's authorized to access.

## How It Works

lazypaw integrates JWT authentication with SQL Server's RLS through this flow:

```
Client → JWT (Bearer token) → lazypaw → SQL Server
                                  │
                                  ├─ 1. Validate JWT (signature, issuer, audience, expiry)
                                  ├─ 2. Map JWT claims to a DB role
                                  ├─ 3. EXECUTE AS USER = 'mapped_role'
                                  ├─ 4. sp_set_session_context (user_id, tenant_id, etc.)
                                  ├─ 5. Execute query — RLS policies filter rows automatically
                                  └─ 6. REVERT (restore service account context)
```

1. **Client sends a JWT** obtained from any OIDC provider (Auth0, Entra ID, Keycloak, etc.).
2. **lazypaw validates the JWT** — checks signature, issuer, audience, and expiration.
3. **Claim-to-role mapping** — a claim from the JWT (e.g. `role`) is mapped to a SQL Server database user.
4. **`EXECUTE AS USER`** — lazypaw impersonates the mapped database user for the duration of the request.
5. **`sp_set_session_context`** — lazypaw injects configured JWT claims (like `user_id`, `tenant_id`) into the session context.
6. **RLS policies kick in** — SQL Server security policies use `SESSION_CONTEXT()` to filter rows transparently.
7. **`REVERT`** — after the query completes, lazypaw reverts to the service account.

## SQL Server Setup

### Step 1: Create the Service Login and User

This is the identity lazypaw uses to connect to SQL Server:

```sql
CREATE LOGIN lazypaw_service WITH PASSWORD = 'strong_password';
CREATE USER lazypaw_service FOR LOGIN lazypaw_service;
```

### Step 2: Create Application Roles

These are database users (without logins) that represent your application roles. lazypaw will impersonate them based on JWT claims:

```sql
CREATE USER app_user WITHOUT LOGIN;
CREATE USER app_admin WITHOUT LOGIN;
```

### Step 3: Grant Impersonation

The service account must be allowed to impersonate the application roles:

```sql
GRANT IMPERSONATE ON USER::app_user TO lazypaw_service;
GRANT IMPERSONATE ON USER::app_admin TO lazypaw_service;
```

### Step 4: Grant Table Access to Roles

Grant permissions to each role as appropriate:

```sql
GRANT SELECT, INSERT, UPDATE, DELETE ON dbo.orders TO app_user;
GRANT SELECT, INSERT, UPDATE, DELETE ON dbo.orders TO app_admin;
```

### Step 5: Create the RLS Predicate Function

This inline table-valued function checks whether the row's `user_id` matches the current session context:

```sql
CREATE FUNCTION dbo.fn_user_filter(@user_id NVARCHAR(128))
RETURNS TABLE
WITH SCHEMABINDING
AS
RETURN SELECT 1 AS result
WHERE @user_id = CAST(SESSION_CONTEXT(N'user_id') AS NVARCHAR(128));
```

### Step 6: Create the Security Policy

Attach the predicate function to your table as both a filter (reads) and block (writes) predicate:

```sql
CREATE SECURITY POLICY dbo.policy_orders
ADD FILTER PREDICATE dbo.fn_user_filter(user_id) ON dbo.orders,
ADD BLOCK PREDICATE dbo.fn_user_filter(user_id) ON dbo.orders;
```

## lazypaw Configuration

Configure JWT validation and claim mapping in your `lazypaw.toml`:

```toml
[auth]
issuer = "https://your-provider.auth0.com/"
audience = "api://lazypaw"

[auth.role_map]
"app_admin" = "app_admin"
"app_user" = "app_user"

[auth.claims]
user_id = "sub"
tenant_id = "custom:tenant_id"
email = "email"
```

| Section | Purpose |
|---------|---------|
| `auth.issuer` | Expected JWT issuer — must match the `iss` claim |
| `auth.audience` | Expected JWT audience — must match the `aud` claim |
| `auth.role_map` | Maps JWT role claim values → SQL Server database users |
| `auth.claims` | Maps session context keys → JWT claim paths. These are injected via `sp_set_session_context` |

## Common Patterns

### Multi-Tenant Isolation

Filter rows by `tenant_id` from the JWT:

```sql
CREATE FUNCTION dbo.fn_tenant_filter(@tenant_id NVARCHAR(128))
RETURNS TABLE
WITH SCHEMABINDING
AS
RETURN SELECT 1 AS result
WHERE @tenant_id = CAST(SESSION_CONTEXT(N'tenant_id') AS NVARCHAR(128));

CREATE SECURITY POLICY dbo.policy_tenant_orders
ADD FILTER PREDICATE dbo.fn_tenant_filter(tenant_id) ON dbo.orders,
ADD BLOCK PREDICATE dbo.fn_tenant_filter(tenant_id) ON dbo.orders;
```

### Owner-Only Access

Users can only see and modify their own rows:

```sql
CREATE FUNCTION dbo.fn_owner_filter(@user_id NVARCHAR(128))
RETURNS TABLE
WITH SCHEMABINDING
AS
RETURN SELECT 1 AS result
WHERE @user_id = CAST(SESSION_CONTEXT(N'user_id') AS NVARCHAR(128));
```

### Role-Based: Admins See All, Users See Own

```sql
CREATE FUNCTION dbo.fn_role_filter(@user_id NVARCHAR(128))
RETURNS TABLE
WITH SCHEMABINDING
AS
RETURN SELECT 1 AS result
WHERE
    @user_id = CAST(SESSION_CONTEXT(N'user_id') AS NVARCHAR(128))
    OR USER_NAME() = 'app_admin';
```

Admins impersonate `app_admin`, so `USER_NAME()` returns `'app_admin'` and bypasses the row filter. Regular users only see rows matching their `user_id`.

### Separate Read vs Write Policies

Use FILTER predicates (for SELECT) and BLOCK predicates (for INSERT/UPDATE/DELETE) independently:

```sql
CREATE SECURITY POLICY dbo.policy_orders
ADD FILTER PREDICATE dbo.fn_tenant_filter(tenant_id) ON dbo.orders,
ADD BLOCK PREDICATE dbo.fn_owner_filter(user_id) ON dbo.orders AFTER INSERT,
ADD BLOCK PREDICATE dbo.fn_owner_filter(user_id) ON dbo.orders BEFORE UPDATE,
ADD BLOCK PREDICATE dbo.fn_owner_filter(user_id) ON dbo.orders BEFORE DELETE;
```

This lets users read all rows in their tenant but only write their own.

## Testing

### Get a Test JWT

```bash
TOKEN=$(curl -s -X POST "https://your-auth.com/oauth/token" \
  -H "Content-Type: application/json" \
  -d '{
    "client_id": "your-client-id",
    "client_secret": "your-client-secret",
    "audience": "api://lazypaw",
    "grant_type": "client_credentials"
  }' | jq -r '.access_token')
```

### Query the API

```bash
# Only sees rows matching the JWT's user_id / tenant_id
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/orders

# Insert — blocked if RLS policy rejects
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"product": "Widget", "quantity": 5}' \
  http://localhost:3000/orders
```

### Verify Row Filtering

```bash
# User A's token — should see only User A's orders
curl -H "Authorization: Bearer $TOKEN_A" http://localhost:3000/orders

# User B's token — should see only User B's orders
curl -H "Authorization: Bearer $TOKEN_B" http://localhost:3000/orders

# Admin token — should see all orders
curl -H "Authorization: Bearer $TOKEN_ADMIN" http://localhost:3000/orders
```

## Gotchas and Tips

### SESSION_CONTEXT Values Are NVARCHAR

Always `CAST` when comparing in predicate functions. Forgetting this causes silent type mismatches:

```sql
-- ✅ Correct
WHERE @user_id = CAST(SESSION_CONTEXT(N'user_id') AS NVARCHAR(128))

-- ❌ Wrong — implicit conversion may fail silently
WHERE @user_id = SESSION_CONTEXT(N'user_id')
```

### EXECUTE AS Requires GRANT IMPERSONATE

If you see permission errors, verify that the service account has impersonation rights:

```sql
GRANT IMPERSONATE ON USER::app_user TO lazypaw_service;
```

### Predicate Functions Must Use SCHEMABINDING

SQL Server requires `WITH SCHEMABINDING` on RLS predicate functions. Without it, `CREATE SECURITY POLICY` will fail.

### Disabling RLS for Admin Operations

If your service account needs unrestricted access for migrations or bulk operations, you can exempt it:

```sql
ALTER SECURITY POLICY dbo.policy_orders
WITH (STATE = OFF);
-- Run admin operations
ALTER SECURITY POLICY dbo.policy_orders
WITH (STATE = ON);
```

Or design your predicate to allow the service account through:

```sql
RETURN SELECT 1 AS result
WHERE @user_id = CAST(SESSION_CONTEXT(N'user_id') AS NVARCHAR(128))
    OR USER_NAME() = 'lazypaw_service';
```

### Performance

- **Use inline table-valued functions** (TVFs) for predicates — they get inlined into the query plan.
- **Avoid scalar functions** — they execute row-by-row and destroy performance.
- **Index the filtered columns** (e.g. `user_id`, `tenant_id`) for efficient predicate evaluation.
- The predicate function runs for every row, so keep it simple and fast.
