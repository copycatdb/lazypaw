# API Reference

## Tables & Views

Every table and view in your database becomes a REST endpoint automatically. The default schema (`dbo`) is omitted from URLs; other schemas use the `/<schema>/<table>` pattern.

### GET — Read rows

```bash
# All rows
GET /users

# With filters, column selection, ordering, pagination
GET /users?select=name,email&status=eq.active&order=created_at.desc&limit=10&offset=0
```

### POST — Insert rows

```bash
POST /users
Content-Type: application/json
Prefer: return=representation

{"name": "Alice", "email": "alice@example.com"}
```

Insert multiple rows by sending an array:

```bash
POST /users
Content-Type: application/json
Prefer: return=representation

[
  {"name": "Alice", "email": "alice@example.com"},
  {"name": "Bob", "email": "bob@example.com"}
]
```

### PATCH — Update rows

```bash
PATCH /users?id=eq.42
Content-Type: application/json
Prefer: return=representation

{"status": "inactive"}
```

Updates all rows matching the filter. Always include a filter unless you intend to update every row.

### DELETE — Delete rows

```bash
DELETE /users?id=eq.42
Prefer: return=representation
```

Deletes all rows matching the filter.

## Query Parameters

### select

Choose columns and embed related tables:

```bash
# Specific columns
GET /users?select=id,name,email

# All columns (default)
GET /users?select=*

# With FK embedding
GET /users?select=name,orders(id,total,items(product,qty))
```

### order

```bash
# Ascending (default)
GET /users?order=name

# Explicit direction
GET /users?order=name.asc
GET /users?order=created_at.desc

# Multiple columns
GET /users?order=status.asc,created_at.desc
```

### limit and offset

```bash
GET /users?limit=10&offset=20
```

You can also use the `Range` header:

```bash
GET /users
Range: 0-24
```

This returns rows 0 through 24 (25 rows).

## Filtering

Filters use the `column=operator.value` syntax in query parameters.

### Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `eq` | Equal | `?status=eq.active` |
| `neq` | Not equal | `?status=neq.deleted` |
| `gt` | Greater than | `?age=gt.21` |
| `gte` | Greater than or equal | `?age=gte.21` |
| `lt` | Less than | `?price=lt.100` |
| `lte` | Less than or equal | `?price=lte.100` |
| `like` | LIKE (case-sensitive, `%` wildcard) | `?name=like.%smith%` |
| `ilike` | LIKE (case-insensitive) | `?name=ilike.%smith%` |
| `is` | IS (null, true, false) | `?deleted_at=is.null` |
| `in` | IN list | `?status=in.(active,pending)` |
| `not` | Negate another operator | `?status=not.eq.deleted` |
| `fts` | Full-text search | `?description=fts.adventure` |

### Logical operators

Combine filters with `or` and `and`:

```bash
# OR — match either condition
GET /users?or=(status.eq.active,status.eq.pending)

# AND — match all conditions (default behavior, but explicit grouping available)
GET /users?and=(age.gte.21,age.lte.65)

# Nested
GET /users?or=(and(status.eq.active,age.gte.21),role.eq.admin)
```

By default, multiple query parameters are ANDed together:

```bash
# These are equivalent
GET /users?status=eq.active&age=gte.21
GET /users?and=(status.eq.active,age.gte.21)
```

## FK Embedding

Resolve foreign key relationships as nested JSON in a single request using the `select` parameter:

```bash
# One-to-many: user has many orders
GET /users?select=name,orders(id,total)

# Response:
[
  {
    "name": "Alice",
    "orders": [
      {"id": 1, "total": 59.99},
      {"id": 2, "total": 120.00}
    ]
  }
]

# Many-to-one: order belongs to a user
GET /orders?select=id,total,users(name,email)

# Response:
[
  {
    "id": 1,
    "total": 59.99,
    "users": {"name": "Alice", "email": "alice@example.com"}
  }
]

# Nested embedding: users → orders → items
GET /users?select=name,orders(id,items(product,qty))
```

Many-to-one embeds return a single object (or `null`). One-to-many embeds return an array.

lazypaw discovers relationships from `sys.foreign_keys` at startup — no configuration needed.

## Prefer Headers

Control response behavior with the `Prefer` header.

### return

```bash
# Return inserted/updated/deleted rows (default for mutations)
Prefer: return=representation

# Return nothing (204 No Content)
Prefer: return=minimal

# Return only headers (count in Content-Range)
Prefer: return=headers-only
```

### count

```bash
# Include total count in Content-Range header
Prefer: count=exact
```

Response header: `Content-Range: 0-9/100` (10 rows returned, 100 total).

### resolution (upsert)

```bash
# Upsert: insert or update on conflict
POST /users
Prefer: return=representation, resolution=merge-duplicates
Content-Type: application/json

{"id": 1, "name": "Alice", "score": 100}
```

This generates a T-SQL `MERGE` statement.

### tx (transaction control)

```bash
# Rollback after execution (useful for testing)
Prefer: tx=rollback
```

## Accept Headers

Control response format with the `Accept` header.

| Accept Header | Format | Description |
|---------------|--------|-------------|
| `application/json` (default) | JSON | Array of objects |
| `application/vnd.pgrst.object+json` | JSON | Single object (error if != 1 row) |
| `text/csv` | CSV | Comma-separated values |
| `application/vnd.apache.arrow.stream` | Arrow IPC | Apache Arrow IPC stream |

```bash
# CSV output
curl -H "Accept: text/csv" http://localhost:3000/users

# Single object
curl -H "Accept: application/vnd.pgrst.object+json" 'http://localhost:3000/users?id=eq.42'

# Arrow IPC (for analytics / DataFrame consumption)
curl -H "Accept: application/vnd.apache.arrow.stream" http://localhost:3000/users -o users.arrow
```

## RPC — Stored Procedures

Call stored procedures via `POST /rpc/<procedure_name>`:

```bash
POST /rpc/get_leaderboard
Content-Type: application/json

{"game_id": 1, "top_n": 10}
```

This executes `EXEC [get_leaderboard] @game_id = 1, @top_n = 10` and returns the result set as JSON.

Parameters are passed as named arguments in the JSON body. An empty body or `{}` calls the procedure with no arguments.

```bash
# No arguments
POST /rpc/refresh_materialized_view
```

## Realtime — WebSocket

When started with `--realtime`, lazypaw exposes a WebSocket endpoint at `/realtime` that pushes INSERT, UPDATE, and DELETE events using SQL Server Change Tracking.

### Connect

```
ws://localhost:3000/realtime
ws://localhost:3000/realtime?token=<jwt>
```

### Subscribe

Send a JSON message to subscribe to table changes:

```json
{
  "type": "subscribe",
  "id": "my-sub-1",
  "table": "orders",
  "events": ["INSERT", "UPDATE", "DELETE"],
  "filter": "status=eq.active"
}
```

- `id` — unique subscription ID (you choose it)
- `table` — table name
- `events` — array of event types to listen for
- `filter` — optional PostgREST-style filter

### Receive changes

```json
{
  "type": "INSERT",
  "id": "my-sub-1",
  "table": "orders",
  "record": {"id": 42, "product": "Widget", "total": 59.99}
}
```

### Unsubscribe

```json
{
  "type": "unsubscribe",
  "id": "my-sub-1"
}
```

### Latency

Change Tracking is polled at the interval configured by `--realtime-poll-ms` (default: 200ms). Typical end-to-end latency is 100–300ms.

## OpenAPI

lazypaw auto-generates an OpenAPI spec from your database schema.

- **OpenAPI spec** — `GET /` returns the JSON spec
- **Swagger UI** — browse `http://localhost:3000/swagger` for interactive API docs

The spec includes all tables, views, columns, types, and relationships.

## Error Responses

Errors return a JSON envelope:

```json
{
  "message": "Table not found: dbo.nonexistent",
  "code": "NOT_FOUND",
  "details": null,
  "hint": null
}
```

### Status codes

| Code | Meaning |
|------|---------|
| 200 | OK — GET, PATCH, DELETE success |
| 201 | Created — POST insert success |
| 204 | No Content — `Prefer: return=minimal` |
| 400 | Bad Request — invalid filters, JSON, or parameters |
| 401 | Unauthorized — missing or invalid JWT |
| 403 | Forbidden — role not permitted |
| 404 | Not Found — table/view doesn't exist |
| 406 | Not Acceptable — single object requested but != 1 row |
| 500 | Internal Server Error — SQL error or server failure |
