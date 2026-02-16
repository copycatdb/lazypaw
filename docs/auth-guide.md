# Authentication Guide

## Overview

lazypaw is a **token consumer**, not a token provider. It validates JWTs issued by your identity provider (Auth0, Entra ID, Firebase, Keycloak, Supabase Auth, etc.) and maps them to SQL Server database roles. lazypaw never stores credentials or issues tokens.

The flow:

```
Identity Provider → JWT → Client → lazypaw → SQL Server
                                      │
                                      ├─ Validate JWT (signature, iss, aud, exp)
                                      ├─ Map role claim → DB user
                                      ├─ EXECUTE AS USER = 'mapped_role'
                                      ├─ sp_set_session_context (claims)
                                      ├─ Execute query (RLS filters rows)
                                      └─ REVERT
```

## JWT Validation

### OIDC Discovery (recommended)

lazypaw fetches the JWKS from your provider's `/.well-known/openid-configuration` endpoint and validates RS256/RS384/RS512 signatures automatically:

```bash
lazypaw --auth-mode oidc \
        --oidc-issuer "https://your-provider.auth0.com/" \
        --oidc-audience "api://lazypaw"
```

### HS256 (shared secret)

For simpler setups, use a shared secret:

```bash
lazypaw --jwt-secret "your-256-bit-secret"
```

This automatically sets `--auth-mode jwt-secret`.

### Supported algorithms

- **RS256, RS384, RS512** — via JWKS discovery (OIDC mode)
- **HS256** — via shared secret (`--jwt-secret`)

## Configuration

### CLI args

```bash
lazypaw --auth-mode oidc \
        --oidc-issuer "https://login.microsoftonline.com/TENANT_ID/v2.0" \
        --oidc-audience "api://lazypaw" \
        --role-claim "roles" \
        --context-claims "sub,email,custom:tenant_id" \
        --anon-role anon
```

### Environment variables

```bash
LAZYPAW_AUTH_MODE=oidc
LAZYPAW_OIDC_ISSUER=https://login.microsoftonline.com/TENANT_ID/v2.0
LAZYPAW_OIDC_AUDIENCE=api://lazypaw
LAZYPAW_ROLE_CLAIM=roles
LAZYPAW_CONTEXT_CLAIMS=sub,email,custom:tenant_id
LAZYPAW_ANON_ROLE=anon
```

### TOML config

```toml
[auth]
mode = "oidc"
issuer = "https://login.microsoftonline.com/TENANT_ID/v2.0"
audience = "api://lazypaw"
role_claim = "roles"
anon_role = "anon"
context_claims = ["sub", "email", "custom:tenant_id"]

[auth.role_map]
"app_admin" = "db_admin"
"app_user" = "db_user"
"viewer" = "db_reader"
```

### role_map

Maps JWT role claim values to SQL Server database users. The JWT claim value (left) maps to the database user (right):

```toml
[auth.role_map]
"admin" = "app_admin"      # JWT role "admin" → DB user "app_admin"
"user" = "app_user"        # JWT role "user"  → DB user "app_user"
```

If the role claim value isn't in the map, lazypaw uses it as-is (the JWT role value becomes the DB user name).

## Provider Examples

### Auth0

```toml
[auth]
mode = "oidc"
issuer = "https://your-tenant.auth0.com/"
audience = "api://lazypaw"
role_claim = "https://your-app.com/roles"
context_claims = ["sub", "email"]

[auth.role_map]
"admin" = "app_admin"
"user" = "app_user"
```

Auth0 requires a custom namespace for role claims (e.g. `https://your-app.com/roles`). Set this up via Auth0 Actions or Rules.

### Microsoft Entra ID (Azure AD)

```toml
[auth]
mode = "oidc"
issuer = "https://login.microsoftonline.com/YOUR_TENANT_ID/v2.0"
audience = "api://lazypaw"
role_claim = "roles"
context_claims = ["oid", "preferred_username", "tid"]

[auth.role_map]
"App.Admin" = "app_admin"
"App.User" = "app_user"
```

Configure App Roles in the Entra ID app registration, then assign them to users/groups.

### Firebase Auth

```toml
[auth]
mode = "oidc"
issuer = "https://securetoken.google.com/YOUR_PROJECT_ID"
audience = "YOUR_PROJECT_ID"
role_claim = "role"
context_claims = ["sub", "email"]

[auth.role_map]
"admin" = "app_admin"
"user" = "app_user"
```

Set custom claims via Firebase Admin SDK: `admin.auth().setCustomUserClaims(uid, { role: 'admin' })`.

### Keycloak

```toml
[auth]
mode = "oidc"
issuer = "https://keycloak.example.com/realms/myrealm"
audience = "lazypaw-api"
role_claim = "realm_access.roles"
context_claims = ["sub", "email", "preferred_username"]

[auth.role_map]
"admin" = "app_admin"
"user" = "app_user"
```

Keycloak nests roles under `realm_access.roles` — use dot notation in `role_claim`.

### Supabase Auth

```toml
[auth]
mode = "oidc"
issuer = "https://YOUR_PROJECT.supabase.co/auth/v1"
audience = "authenticated"
role_claim = "role"
context_claims = ["sub", "email"]

[auth.role_map]
"authenticated" = "app_user"
"service_role" = "app_admin"
```

## Claim Mapping

### Dot notation

Access nested claims with dot notation:

```toml
role_claim = "realm_access.roles"         # Keycloak
role_claim = "https://app.com/roles"      # Auth0 namespace
context_claims = ["sub", "custom:tenant_id", "address.city"]
```

### Arrays

If the role claim is an array (e.g. Entra ID `roles: ["App.Admin", "App.User"]`), lazypaw uses the first matching value from `role_map`.

### Defaults

If a claim is missing from the JWT, it's silently skipped in session context. If the role claim is missing and `anon_role` is configured, the request proceeds as the anonymous role.

## DB Impersonation

lazypaw uses SQL Server's `EXECUTE AS USER` to impersonate the mapped database role for each request:

```sql
-- What lazypaw runs per request:
EXECUTE AS USER = 'app_user';
EXEC sp_set_session_context N'user_id', N'auth0|abc123';
EXEC sp_set_session_context N'tenant_id', N'tenant_42';

-- Your query runs here (RLS policies filter rows)

REVERT;
```

### Setup

```sql
-- Create roles (database users without logins)
CREATE USER app_user WITHOUT LOGIN;
CREATE USER app_admin WITHOUT LOGIN;

-- Grant impersonation to the service account
GRANT IMPERSONATE ON USER::app_user TO lazypaw_service;
GRANT IMPERSONATE ON USER::app_admin TO lazypaw_service;

-- Grant table access
GRANT SELECT, INSERT, UPDATE, DELETE ON dbo.orders TO app_user;
GRANT SELECT, INSERT, UPDATE, DELETE ON dbo.orders TO app_admin;
```

Or use `lazypaw setup` to generate these scripts:

```bash
lazypaw setup --server host --database db --roles "app_user,app_admin" --service-account lazypaw_service
```

## Session Context

JWT claims listed in `context_claims` are injected via `sp_set_session_context`. Use them in RLS predicate functions:

```sql
-- In your RLS predicate function
WHERE user_id = CAST(SESSION_CONTEXT(N'user_id') AS NVARCHAR(128))
```

See the [RLS Guide](./rls-guide.md) for complete patterns.

## Anonymous Access

Set `anon_role` to allow unauthenticated requests:

```bash
lazypaw --anon-role anon
```

```sql
-- Create the anon role with limited permissions
CREATE USER anon WITHOUT LOGIN;
GRANT SELECT ON dbo.public_posts TO anon;
-- No INSERT/UPDATE/DELETE = read-only public access
```

Requests without a JWT (or with an invalid JWT when `anon_role` is set) execute as the anonymous role.

## Azure Managed Identity

Connect to Azure SQL without passwords using Managed Identity or Workload Identity:

```bash
# Azure VM / Container Apps / AKS with Managed Identity
lazypaw --server mydb.database.windows.net \
        --database mydb \
        --db-auth managed-identity

# Service Principal
lazypaw --server mydb.database.windows.net \
        --database mydb \
        --db-auth service-principal \
        --sp-tenant-id YOUR_TENANT \
        --sp-client-id YOUR_CLIENT_ID \
        --sp-client-secret YOUR_SECRET
```

lazypaw acquires an Azure AD token via IMDS (Managed Identity) or client credentials (Service Principal) and uses it to authenticate to SQL Server. No password needed.

## Testing

### Get a JWT from your provider

```bash
# Auth0
TOKEN=$(curl -s -X POST "https://your-tenant.auth0.com/oauth/token" \
  -H "Content-Type: application/json" \
  -d '{
    "client_id": "YOUR_CLIENT_ID",
    "client_secret": "YOUR_CLIENT_SECRET",
    "audience": "api://lazypaw",
    "grant_type": "client_credentials"
  }' | jq -r '.access_token')
```

### Make authenticated requests

```bash
# Read — RLS filters rows based on JWT claims
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/orders

# Write
curl -X POST http://localhost:3000/orders \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -H "Prefer: return=representation" \
  -d '{"product": "Widget", "quantity": 5}'

# Verify different roles see different data
curl -H "Authorization: Bearer $USER_TOKEN" http://localhost:3000/orders    # user's orders only
curl -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:3000/orders   # all orders
```

### Test with HS256 secret

For development, you can generate JWTs locally using the shared secret:

```bash
lazypaw --jwt-secret "dev-secret-at-least-32-characters-long"

# Generate a test JWT (using jwt-cli or jwt.io)
TOKEN=$(jwt encode --secret "dev-secret-at-least-32-characters-long" \
  '{"sub": "user-1", "role": "app_user", "email": "test@example.com"}')

curl -H "Authorization: Bearer $TOKEN" http://localhost:3000/users
```
