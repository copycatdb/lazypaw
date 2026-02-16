
## Features Already Implemented

- **Upsert (MERGE)**: `Prefer: resolution=merge-duplicates` header on POST triggers T-SQL MERGE (#32)
- **RPC (Stored Procedures)**: `POST /rpc/:proc_name` with JSON body params (#33)
- **Exact Count**: `Prefer: count=exact` header returns total count in Content-Range (#34)
- **Single Object Response**: `Accept: application/vnd.pgrst.object+json` returns single object or 406 (#35)
