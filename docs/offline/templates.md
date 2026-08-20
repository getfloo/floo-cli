floo Templates - Copy-Paste App Structures

## React + FastAPI (Multi-Service)

  A frontend (React/Vite) + backend (FastAPI) app with a shared database.

### Directory Structure

  my-app/
  ├── floo.app.toml          # single config file - services declared inline
  ├── Dockerfile
  ├── web/                   # React frontend
  │   ├── package.json
  │   └── src/
  └── api/                   # FastAPI backend
      ├── pyproject.toml     # or requirements.txt
      └── app/
          └── main.py

### floo.app.toml (single file for the whole app)

  [app]
  name = "my-app"

  [managed.default]
  type = "postgres"

  [services.web]
  path = "./web"
  type = "web"
  port = 3000
  ingress = "public"
  dev_command = "npm run dev"

  [services.api]
  path = "./api"
  type = "api"
  port = 8080
  ingress = "public"
  dev_command = "uvicorn app.main:app --reload --port 8080"
  migrate_command = "alembic upgrade head"

  [services.api.env]
  managed = ["postgres"]

  [services.web.env]
  managed = []

### api/app/main.py

  from fastapi import FastAPI, Request

  app = FastAPI()

  @app.get("/users")
  async def list_users(request: Request):
      # The gateway routes /api/users → /users (strips the /api prefix)
      # Identity headers are injected when access_mode != "public":
      user_email = request.headers.get("X-Floo-User-Email")
      return {"users": [], "requested_by": user_email}

  @app.get("/health")
  async def health():
      return {"status": "ok"}

### web/src/App.tsx (React calling the API)

  // In production, the gateway routes /api/* to the api service.
  // No CORS needed - same origin.
  const response = await fetch("/api/users");
  const data = await response.json();

  // For local development, proxy /api/* to the FastAPI dev server.
  // In vite.config.ts:
  //   server: { proxy: { "/api": "http://localhost:8080" } }

### Deploy

  PREREQUISITE: Your code must be in a GitHub repo. floo pulls source
  from GitHub - it does not upload local files.

  1. floo auth login
  2. floo init my-app                          # from root directory
  3. floo preflight                            # validate both services
  4. git add . && git commit -m "chore: configure floo"
  5. git push origin main                      # push config before connecting
  6. floo apps github connect owner/my-app     # creates app and deploys pushed source
  7. floo apps show my-app                     # get your URL

### Local Development (two terminals)

  Terminal 1 (backend):
    cd api && uvicorn app.main:app --reload --port 8080

  Terminal 2 (frontend):
    cd web && npm run dev
    # vite.config.ts proxy forwards /api/* to localhost:8080

  Or use floo dev for cloud-connected local development:
  floo dev --app my-app                  # starts both services

### Env Vars

  Backend secrets (api service only):
    floo env set SECRET_KEY --stdin --secret --service api

  DATABASE_URL is injected from the default `postgres` attachment.

  Frontend config (web service only, public - baked into JS bundle):
    floo env set VITE_API_URL=/api --service web

  SECURITY: Never set backend secrets on the web service.
  Build-time vars (VITE_*, NEXT_PUBLIC_*) are visible to end users.

## Next.js + FastAPI (Multi-Service)

  Same structure as above - declare both services inline in one floo.app.toml.
  Replace the [services.web] entry with a Next.js service:

  [services.web]
  path = "./web"
  type = "web"
  port = 3000
  ingress = "public"
  dev_command = "npm run dev"

### Key Differences from React

  - Next.js API routes can also call the FastAPI service internally
  - Use NEXT_PUBLIC_* prefix for client-side env vars (same security rules)
  - Server components can read X-Floo-User-* headers directly

## Single-Service App (Simplest)

  For a standalone app (just a web server, no separate API):

  my-app/
  ├── floo.app.toml
  ├── Dockerfile
  ├── package.json
  └── src/

  floo.app.toml:

  [app]
  name = "my-app"

  [services.web]
  path = "."
  type = "web"
  port = 3000
  ingress = "public"
  dev_command = "npm run dev"

  Deploy:
    floo auth login
    floo init my-app
    floo preflight
    git add . && git commit -m "chore: configure floo"
    git push origin main
    floo apps github connect owner/my-app
