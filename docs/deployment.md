# Deployment Guide

## Docker

```bash
docker run -p 3000:3000 copycatdb/lazypaw \
  --server host.docker.internal \
  --database mydb \
  --user api_service \
  --password "$DB_PASSWORD"
```

### Environment variables

```bash
docker run -p 3000:3000 \
  -e LAZYPAW_SERVER=host.docker.internal \
  -e LAZYPAW_DATABASE=mydb \
  -e LAZYPAW_USER=api_service \
  -e LAZYPAW_PASSWORD_FILE=/run/secrets/db_password \
  copycatdb/lazypaw
```

### Health check

```dockerfile
HEALTHCHECK --interval=30s --timeout=3s \
  CMD curl -f http://localhost:3000/ || exit 1
```

The root endpoint (`GET /`) returns the OpenAPI spec — a 200 response means lazypaw is running and connected to the database.

## Docker Compose

```yaml
# docker-compose.yml
services:
  lazypaw:
    image: copycatdb/lazypaw
    ports:
      - "3000:3000"
    environment:
      LAZYPAW_SERVER: sqlserver
      LAZYPAW_DATABASE: mydb
      LAZYPAW_USER: api_service
      LAZYPAW_PASSWORD_FILE: /run/secrets/db_password
      LAZYPAW_AUTH_MODE: oidc
      LAZYPAW_OIDC_ISSUER: "https://your-provider.auth0.com/"
      LAZYPAW_OIDC_AUDIENCE: "api://lazypaw"
      LAZYPAW_ANON_ROLE: anon
      LAZYPAW_REALTIME: "true"
    secrets:
      - db_password
    depends_on:
      - sqlserver
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/"]
      interval: 30s
      timeout: 3s
      retries: 3

  sqlserver:
    image: mcr.microsoft.com/mssql/server:2022-latest
    environment:
      ACCEPT_EULA: "Y"
      MSSQL_SA_PASSWORD: "YourStrong!Pass"
    ports:
      - "1433:1433"

secrets:
  db_password:
    file: ./db_password.txt
```

## Azure

### Container Apps

```bash
az containerapp create \
  --name lazypaw \
  --resource-group mygroup \
  --image copycatdb/lazypaw \
  --target-port 3000 \
  --ingress external \
  --env-vars \
    LAZYPAW_SERVER=mydb.database.windows.net \
    LAZYPAW_DATABASE=mydb \
    LAZYPAW_DB_AUTH=managed-identity \
    LAZYPAW_AUTH_MODE=oidc \
    LAZYPAW_OIDC_ISSUER="https://login.microsoftonline.com/TENANT_ID/v2.0" \
    LAZYPAW_OIDC_AUDIENCE="api://lazypaw"
```

Enable system-assigned managed identity and grant it access to your Azure SQL database:

```sql
CREATE USER [lazypaw-container-app] FROM EXTERNAL PROVIDER;
ALTER ROLE db_datareader ADD MEMBER [lazypaw-container-app];
ALTER ROLE db_datawriter ADD MEMBER [lazypaw-container-app];
```

### AKS (Kubernetes)

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: lazypaw
spec:
  replicas: 2
  selector:
    matchLabels:
      app: lazypaw
  template:
    metadata:
      labels:
        app: lazypaw
    spec:
      containers:
        - name: lazypaw
          image: copycatdb/lazypaw
          ports:
            - containerPort: 3000
          env:
            - name: LAZYPAW_SERVER
              value: mydb.database.windows.net
            - name: LAZYPAW_DATABASE
              value: mydb
            - name: LAZYPAW_DB_AUTH
              value: managed-identity
            - name: LAZYPAW_POOL_SIZE
              value: "20"
          livenessProbe:
            httpGet:
              path: /
              port: 3000
            initialDelaySeconds: 5
            periodSeconds: 30
---
apiVersion: v1
kind: Service
metadata:
  name: lazypaw
spec:
  selector:
    app: lazypaw
  ports:
    - port: 80
      targetPort: 3000
  type: ClusterIP
```

Use Workload Identity for passwordless Azure SQL access from AKS.

## AWS

### Fargate / ECS

```json
{
  "family": "lazypaw",
  "containerDefinitions": [
    {
      "name": "lazypaw",
      "image": "copycatdb/lazypaw",
      "portMappings": [{"containerPort": 3000}],
      "environment": [
        {"name": "LAZYPAW_SERVER", "value": "mydb.rds.amazonaws.com"},
        {"name": "LAZYPAW_DATABASE", "value": "mydb"},
        {"name": "LAZYPAW_USER", "value": "api_service"},
        {"name": "LAZYPAW_POOL_SIZE", "value": "10"}
      ],
      "secrets": [
        {"name": "LAZYPAW_PASSWORD", "valueFrom": "arn:aws:secretsmanager:us-east-1:123456:secret:db-password"}
      ],
      "healthCheck": {
        "command": ["CMD-SHELL", "curl -f http://localhost:3000/ || exit 1"],
        "interval": 30,
        "timeout": 5,
        "retries": 3
      }
    }
  ],
  "cpu": "256",
  "memory": "512"
}
```

## GCP

### Cloud Run

```bash
gcloud run deploy lazypaw \
  --image copycatdb/lazypaw \
  --port 3000 \
  --set-env-vars "LAZYPAW_SERVER=10.0.0.1,LAZYPAW_DATABASE=mydb,LAZYPAW_USER=api_service" \
  --set-secrets "LAZYPAW_PASSWORD=db-password:latest" \
  --allow-unauthenticated \
  --vpc-connector my-connector
```

Note: Cloud Run cold starts are fast (lazypaw starts in milliseconds), but keep min-instances ≥ 1 for realtime WebSocket connections.

## Fly.io

```toml
# fly.toml
app = "lazypaw"
primary_region = "iad"

[build]
  image = "copycatdb/lazypaw"

[env]
  LAZYPAW_SERVER = "mydb.database.windows.net"
  LAZYPAW_DATABASE = "mydb"
  LAZYPAW_LISTEN_PORT = "8080"

[http_service]
  internal_port = 8080
  force_https = true

[[services.http_checks]]
  interval = 30000
  timeout = 5000
  path = "/"
```

```bash
fly secrets set LAZYPAW_PASSWORD=yourpassword
fly deploy
```

## Bare Metal

### systemd

```ini
# /etc/systemd/system/lazypaw.service
[Unit]
Description=lazypaw REST API
After=network.target

[Service]
Type=simple
User=lazypaw
ExecStart=/usr/local/bin/lazypaw --config /etc/lazypaw/lazypaw.toml
Restart=always
RestartSec=5
Environment=LAZYPAW_PASSWORD_FILE=/etc/lazypaw/db_password

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable lazypaw
sudo systemctl start lazypaw
```

## Reverse Proxy

### nginx

```nginx
upstream lazypaw {
    server 127.0.0.1:3000;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate /etc/ssl/certs/api.pem;
    ssl_certificate_key /etc/ssl/private/api.key;

    location / {
        proxy_pass http://lazypaw;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # WebSocket support for /realtime
    location /realtime {
        proxy_pass http://lazypaw;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_read_timeout 86400;
    }
}
```

### Caddy

```
api.example.com {
    reverse_proxy localhost:3000
}
```

Caddy automatically handles TLS, HTTP/2, and WebSocket upgrades.

## TLS

lazypaw doesn't terminate TLS itself — use a reverse proxy (nginx, Caddy, cloud load balancer) for TLS termination. This is the standard pattern and keeps lazypaw simple.

For SQL Server connections, use `--trust-cert` only in development. In production, ensure your SQL Server has a valid certificate.

## Connection Pooling

lazypaw maintains a connection pool to SQL Server. Tune it based on your workload:

```bash
lazypaw --pool-size 20   # default: 10
```

Guidelines:
- **Low traffic / dev**: 5–10
- **Moderate traffic**: 10–20
- **High traffic**: 20–50
- Don't exceed SQL Server's max connections (default: 32,767, but practical limits are lower)

Each connection uses `EXECUTE AS USER` / `REVERT` per request — connections are safely shared across users.

## Security Checklist

- [ ] **Use a dedicated service account** — not `sa`, not `dbo`. Create a `lazypaw_service` login with only the required permissions.
- [ ] **Enable JWT authentication** — don't expose lazypaw without auth in production.
- [ ] **Set up `anon_role` carefully** — only grant SELECT on public tables, never INSERT/UPDATE/DELETE.
- [ ] **Use RLS** — `sp_set_session_context` + security policies for row-level isolation.
- [ ] **TLS everywhere** — reverse proxy with TLS for client connections, encrypted SQL Server connections.
- [ ] **Use `LAZYPAW_PASSWORD_FILE`** — don't put passwords in environment variables visible to `ps` or `/proc`.
- [ ] **Limit exposed schemas** — use `--schemas` to restrict which schemas are exposed as API endpoints.
- [ ] **Run `lazypaw setup`** — generates the SQL setup script. Review it before running.
- [ ] **Firewall** — only allow traffic from your reverse proxy to lazypaw, and from lazypaw to SQL Server.
- [ ] **Monitor** — lazypaw logs to stdout. Ship logs to your observability stack.
