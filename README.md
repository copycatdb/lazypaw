# lazypaw 😴

Instant REST API from your SQL Server database. Minimal effort. Maximum nap.

Part of [CopyCat](https://github.com/copycatdb) 🐱

## What is this?

Point lazypaw at a SQL Server database and it generates a full REST API. No code. No ORM. No backend. Just your tables, served on a silver platter.

Like [PostgREST](https://github.com/PostgREST/postgrest), but for SQL Server. Because your database already has the data, the schemas, the relationships, and the permissions. Why build another layer?

```bash
# Thats it. Thats the whole setup.
lazypaw --server localhost,1433 --user sa --password pass --database mydb --port 3000
```

```bash
# Every table is now a REST endpoint
GET    /users              # list all users
GET    /users?id=eq.42     # filter
POST   /users              # insert
PATCH  /users?id=eq.42     # update
DELETE /users?id=eq.42     # delete

# Relationships
GET    /users?select=*,orders(*)

# Pagination
GET    /users?limit=10&offset=20&order=name.asc
```

## Why?

Because sometimes you just need a CRUD API and you dont want to write 47 Express routes, a Prisma schema, 12 DTOs, and a controller layer just to read from a table.

lazypaw is for:
- Internal tools and dashboards
- Prototypes and hackathons
- Admin panels
- "I need an API by EOD" situations
- People who value their weekends

## Architecture

```
HTTP Request → lazypaw (Rust) → tabby 🐱 → SQL Server
                  |
                  ├── Schema introspection (on startup)
                  ├── Route generation (automatic)
                  ├── Query building (from URL params)
                  └── JSON serialization (from Arrow buffers)
```

## Status

🚧 Coming soon. Nap in progress.

## Attribution

Shamelessly copied from [PostgREST](https://github.com/PostgREST/postgrest). Theyre the ones who had the genius idea. Were just the cat that brought it home.

## License

MIT
