# SDK Reference — lazypaw-js

## Installation

```bash
npm install lazypaw-js
```

## createClient

```typescript
import { createClient } from 'lazypaw-js'

// Basic
const lp = createClient('http://localhost:3000')

// With static token
const lp = createClient('http://localhost:3000', {
  token: 'your-jwt-token',
})

// With token function (called before each request)
const lp = createClient('http://localhost:3000', {
  tokenFn: async () => {
    const token = await getTokenFromAuthProvider()
    return token
  },
})

// With API key
const lp = createClient('http://localhost:3000', {
  apiKey: 'your-api-key',
})

// With custom headers
const lp = createClient('http://localhost:3000', {
  token: 'your-jwt',
  headers: { 'X-Custom-Header': 'value' },
})
```

### Options

| Option | Type | Description |
|--------|------|-------------|
| `token` | `string` | Static JWT token, sent as `Authorization: Bearer <token>` |
| `tokenFn` | `() => Promise<string \| null>` | Async function called before each request to get a fresh token |
| `apiKey` | `string` | API key sent as `apikey` header |
| `headers` | `Record<string, string>` | Custom headers added to every request |

## QueryBuilder

Start a query with `lp.from('table')`. The query builder is chainable and executes when awaited.

### select

```typescript
// All columns
const { data } = await lp.from('users').select('*')

// Specific columns
const { data } = await lp.from('users').select('id, name, email')

// With FK embedding
const { data } = await lp.from('users').select('name, orders(id, total)')

// Nested embedding
const { data } = await lp.from('users').select('name, orders(id, items(product, qty))')
```

### Filters

```typescript
// Equal
.eq('status', 'active')             // status=eq.active

// Not equal
.neq('status', 'deleted')           // status=neq.deleted

// Greater than / greater than or equal
.gt('age', 21)                      // age=gt.21
.gte('age', 21)                     // age=gte.21

// Less than / less than or equal
.lt('price', 100)                   // price=lt.100
.lte('price', 100)                  // price=lte.100

// Pattern matching
.like('name', '%smith%')            // name=like.%smith%
.ilike('name', '%smith%')           // name=ilike.%smith%

// Null / boolean check
.is('deleted_at', 'null')           // deleted_at=is.null
.is('active', 'true')               // active=is.true

// IN list
.in('status', ['active', 'pending']) // status=in.(active,pending)

// Full-text search
.textSearch('description', 'game')   // description=fts.game

// Negate
.not('status', 'eq', 'deleted')     // status=not.eq.deleted
```

### order

```typescript
// Ascending (default)
.order('name')

// Descending
.order('created_at', { ascending: false })
```

### limit and offset

```typescript
.limit(10)
.offset(20)
```

### range

```typescript
// Get rows 0-9 (first 10)
.range(0, 9)
```

### single

Returns a single object instead of an array. Errors if not exactly 1 row.

```typescript
const { data } = await lp.from('users').select('*').eq('id', 42).single()
// data: { id: 42, name: 'Alice', ... }  (object, not array)
```

### maybeSingle

Like `single()` but returns `null` instead of an error when 0 rows match.

```typescript
const { data } = await lp.from('users').select('*').eq('id', 999).maybeSingle()
// data: null  (no error)
```

### count

Request the total count via `Content-Range` header:

```typescript
const { data, count } = await lp.from('users')
  .select('*')
  .eq('status', 'active')
  .count()

console.log(count) // 142 (total matching rows)
console.log(data)  // first page of results
```

## Mutations

### insert

```typescript
// Single row
const { data, error } = await lp.from('users')
  .insert({ name: 'Alice', email: 'alice@example.com' })

// Multiple rows
const { data, error } = await lp.from('users')
  .insert([
    { name: 'Alice', email: 'alice@example.com' },
    { name: 'Bob', email: 'bob@example.com' },
  ])
```

`insert` automatically sends `Prefer: return=representation` — the inserted rows are returned in `data`.

### upsert

Insert or update on primary key conflict (generates T-SQL `MERGE`):

```typescript
const { data, error } = await lp.from('users')
  .upsert({ id: 1, name: 'Alice', score: 100 })
```

Sends `Prefer: return=representation, resolution=merge-duplicates`.

### update

```typescript
const { data, error } = await lp.from('users')
  .update({ status: 'inactive' })
  .eq('id', 42)
```

Always chain a filter — otherwise all rows are updated.

### delete

```typescript
const { data, error } = await lp.from('users')
  .delete()
  .eq('id', 42)
```

Always chain a filter — otherwise all rows are deleted.

## RPC — Stored Procedures

```typescript
const { data, error } = await lp.rpc('get_leaderboard', { game_id: 1, top_n: 10 })
```

Calls `EXEC [get_leaderboard] @game_id = 1, @top_n = 10` and returns the result set.

```typescript
// No arguments
const { data } = await lp.rpc('refresh_cache')
```

## Realtime

### channel

```typescript
const channel = lp.channel('orders')
```

### on

Listen for change events:

```typescript
// Simple syntax
channel.on('INSERT', (payload) => {
  console.log('New order:', payload.record)
})

channel.on('UPDATE', (payload) => {
  console.log('Updated:', payload.record)
})

channel.on('DELETE', (payload) => {
  console.log('Deleted:', payload.record)
})

// All events
channel.on('*', (payload) => {
  console.log(`${payload.type}:`, payload.record)
})

// Supabase-compatible syntax
channel.on('postgres_changes', {
  event: 'INSERT',
  table: 'orders',
  filter: 'status=eq.active',
}, (payload) => {
  console.log('New active order:', payload.record)
})
```

### subscribe

Start receiving events:

```typescript
channel.subscribe()
```

### unsubscribe

```typescript
channel.unsubscribe()
```

### Full example

```typescript
const channel = lp.channel('orders')
  .on('INSERT', (payload) => console.log('New:', payload.record))
  .on('UPDATE', (payload) => console.log('Changed:', payload.record))
  .subscribe()

// Later...
channel.unsubscribe()
lp.disconnect() // Close all WebSocket connections
```

### Change event shape

```typescript
{
  type: 'INSERT' | 'UPDATE' | 'DELETE',
  table: string,
  record: T
}
```

## Auth

### setToken

Update the token for all subsequent requests:

```typescript
lp.setToken('new-jwt-token')

// Clear token (switch to anonymous)
lp.setToken(null)
```

### onAuthStateChange

Listen for auth state changes:

```typescript
const { unsubscribe } = lp.onAuthStateChange((event, token) => {
  console.log(event) // 'SIGNED_IN' | 'SIGNED_OUT' | 'TOKEN_REFRESHED'
  console.log(token) // JWT string or null
})

// Stop listening
unsubscribe()
```

Events fire when:
- `setToken(token)` is called with a non-null token → `SIGNED_IN` (first time) or `TOKEN_REFRESHED`
- `setToken(null)` is called → `SIGNED_OUT`
- `tokenFn` returns a new token → `TOKEN_REFRESHED`

## Type Safety with Codegen

Generate types, then pass them to `createClient`:

```bash
lazypaw codegen --lang typescript --output ./src/db-types.ts
```

```typescript
import type { Database } from './db-types'
import { createClient } from 'lazypaw-js'

const lp = createClient<Database>('http://localhost:3000')

// Full autocomplete and type checking
const { data } = await lp.from('users').select('*')
//    ^? Database['users']['Row'][] | null

await lp.from('users').insert({ name: 'Alice' })
//                              ^? Database['users']['Insert']
```

## Error Handling

Every query returns `{ data, error, count? }`:

```typescript
const { data, error, count } = await lp.from('users').select('*').count()

if (error) {
  console.error(error.message)  // Human-readable message
  console.error(error.code)     // Error code (e.g. 'NOT_FOUND')
  console.error(error.details)  // Additional details
  console.error(error.hint)     // Suggestion for fixing
  return
}

// data is non-null here
console.log(data)
```

### Error shape

```typescript
interface LazypawError {
  message: string
  code?: string
  details?: string
  hint?: string
}
```

## Migration from Supabase

lazypaw-js is designed to be API-compatible with supabase-js. Here's what's the same and what's different.

### What's the same

```typescript
// These work identically in both libraries:
lp.from('table').select('*')
lp.from('table').select('*, related(*)').eq('id', 1).single()
lp.from('table').insert({ ... })
lp.from('table').upsert({ ... })
lp.from('table').update({ ... }).eq('id', 1)
lp.from('table').delete().eq('id', 1)
lp.rpc('function_name', { args })
lp.channel('table').on('INSERT', cb).subscribe()
```

### What's different

| Feature | supabase-js | lazypaw-js |
|---------|-------------|------------|
| Auth (signup/login) | Built-in `supabase.auth` | Not included — use your OIDC provider directly |
| Storage | `supabase.storage` | Not included |
| Edge Functions | `supabase.functions` | Not included |
| Database | PostgreSQL | SQL Server |
| Realtime mechanism | PostgreSQL LISTEN/NOTIFY | SQL Server Change Tracking |

### Migration steps

1. `npm install lazypaw-js`
2. Replace `import { createClient } from '@supabase/supabase-js'` with `import { createClient } from 'lazypaw-js'`
3. Update the URL to your lazypaw instance
4. Remove `supabase.auth`, `supabase.storage`, `supabase.functions` calls
5. All query builder code works as-is
