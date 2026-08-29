floo - Build a FastAPI app on floo

End-to-end FastAPI journey: deploy, add Postgres, add per-user auth,
add a custom domain. Every step has runnable Python code in the
published guide.

## 1. Dockerfile

  FROM python:3.12-slim
  ...
  CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]

Bind to 0.0.0.0 - floo cannot reach 127.0.0.1.

## 2. floo init + deploy

  floo init my-fastapi-app

  [services.web]
  type = "web"
  path = "."
  port = 8000
  ingress = "public"
  dev_command = "uvicorn app.main:app --reload --port 8000"
  migrate_command = "alembic upgrade head"   # if you use Alembic

  git push origin main
  floo apps github connect owner/my-fastapi-app

## 3. Postgres

  [managed.primary]
  type = "postgres"
  tier = "basic"

  # Async SQLAlchemy - convert to asyncpg URL:
  DATABASE_URL = os.environ["DATABASE_URL"].replace(
      "postgresql://", "postgresql+asyncpg://", 1)

## 4. Per-user auth - pick a model

  [app]
  access_mode = "accounts"

Then a FastAPI dependency:

  def require_user(
      email: Annotated[str | None, Header(alias="X-Floo-User-Email")] = None,
      user_id: Annotated[str | None, Header(alias="X-Floo-User-Id")] = None,
  ):
      if not email: raise HTTPException(401)
      return FlooUser(email=email, user_id=user_id)

  @app.get("/dashboard")
  async def dashboard(user = Depends(require_user)): ...

## 5. Custom domain

  floo domains add app.example.com --app my-fastapi-app

## 6. Local dev

  floo dev --app my-fastapi-app

## Gotchas

  - /healthz is reserved by floo - use /health
  - Bind to 0.0.0.0
  - Don't mix asyncpg and psycopg2 - pick one
  - X-Forwarded-Proto: build absolute URLs from forwarded scheme

Full guide with complete Python code: https://getfloo.com/docs/build/fastapi
