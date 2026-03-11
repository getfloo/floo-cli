---
name: floo-security
description: >
  Security rules for Floo applications. Use when writing application code
  that handles authentication, database connections, secrets, environment
  variables, cookies, CORS, or when deploying to production. Always active
  in Floo projects.
---

# Floo Security Rules

These rules apply to all code written for applications deployed on Floo. They are not suggestions — they are requirements. Violating them creates security vulnerabilities.

## Secrets Management

**Rules:**
- NEVER hardcode credentials, API keys, tokens, or connection strings in source code
- NEVER commit `.env` files to git — add `.env` to `.gitignore` before first commit
- NEVER log secret values — log the key name only (e.g., `"Setting DATABASE_URL"` not the URL itself)
- NEVER expose secrets in error messages, stack traces, or API responses
- NEVER store secrets in `floo.app.toml` or `floo.service.toml` — these are committed to git

**Correct pattern:**
- Store secrets via `floo env set KEY=value --app my-app`
- Import from `.env` file via `floo env import .env --app my-app` (the file stays local, values go to Floo encrypted)
- Read in code via `os.environ["KEY"]` (Python) / `process.env.KEY` (Node.js) / `std::env::var("KEY")` (Rust)
- For local dev, use a `.env` file that is in `.gitignore`

## Database Security

**Rules:**
- ALWAYS use the Floo-provisioned `DATABASE_URL` — never construct your own connection string
- NEVER use superuser or admin credentials in application code
- ALWAYS use parameterized queries — never string concatenation or interpolation for SQL
- NEVER expose database errors directly to users — catch and return generic messages
- NEVER reuse `DATABASE_URL` across environments — dev and prod must have separate credentials

**Anti-patterns to reject:**

```python
# BAD — hardcoded credentials
db_url = "postgresql://admin:password@host:5432/mydb"

# BAD — SQL injection via string formatting
query = f"SELECT * FROM users WHERE id = {user_id}"
cursor.execute(f"DELETE FROM posts WHERE id = {post_id}")

# BAD — exposing DB errors to users
except Exception as e:
    return {"error": str(e)}  # leaks schema, table names, query structure
```

```javascript
// BAD — SQL injection via template literal
const query = `SELECT * FROM users WHERE id = ${userId}`;

// BAD — hardcoded connection
const pool = new Pool({ connectionString: "postgresql://admin:pass@host/db" });
```

**Correct:**

```python
# GOOD — parameterized query
cursor.execute("SELECT * FROM users WHERE id = %s", (user_id,))

# GOOD — env-based connection
database_url = os.environ["DATABASE_URL"]

# GOOD — generic error response
except Exception:
    return {"error": "An internal error occurred"}
```

## Authentication & Sessions

**Rules:**
- Set `HttpOnly`, `Secure`, `SameSite=Lax` on all authentication cookies
- NEVER store auth tokens in `localStorage` — use `HttpOnly` cookies
- Validate JWTs on the server side — never trust client-side token claims
- Set session expiration — no infinite sessions
- Hash passwords with bcrypt (cost >= 12) or argon2 — never plaintext, MD5, or SHA

## CORS & Headers

**Rules:**
- Set CORS `Access-Control-Allow-Origin` to specific origins, NEVER `*` in production
- Set `X-Content-Type-Options: nosniff`
- Set `X-Frame-Options: DENY` (or `SAMEORIGIN` if iframes are needed)
- Set `Strict-Transport-Security` header for HTTPS enforcement

**Anti-pattern:**

```python
# BAD — wide-open CORS in production
CORS(app, origins=["*"])
app.add_middleware(CORSMiddleware, allow_origins=["*"])
```

```javascript
// BAD
app.use(cors({ origin: "*" }));
```

## Deploy Safety

**Rules:**
- NEVER deploy with debug mode enabled in production (`DEBUG=True`, `NODE_ENV=development`)
- Verify environment variables differ between dev and prod: `floo env list --app my-app`
- Use `floo deploy --dry-run` before production deploys
- Review build logs after deploy: `floo deploy logs <id> --app my-app`
- Check runtime logs after deploy: `floo logs --app my-app --since 5m --error`

## Anti-Pattern Blocklist

The following patterns must NEVER appear in application code deployed on Floo:

| Pattern | Risk |
|---------|------|
| `password = "..."` or `api_key = "..."` in source | Hardcoded credential |
| `CORS(allow_origins=["*"])` or `cors({ origin: "*" })` in production | Open CORS |
| `DEBUG=True` / `NODE_ENV=development` in prod environment | Debug mode in production |
| SQL with f-strings, `.format()`, or template literals | SQL injection |
| `.env` file without `.gitignore` entry | Secrets in version control |
| `console.log(secret)` / `print(password)` / `log.info(token)` | Secret in logs |
| `localStorage.setItem("token", ...)` | Token exposed to XSS |
| Auth cookie without `httpOnly: true` | Cookie exposed to XSS |
| `bcrypt` with cost factor < 10 | Weak password hashing |
| `eval()` / `exec()` with user input | Code injection |
| Connection string containing `admin` or `superuser` role | Over-privileged DB access |
| `FLOO_SECRET_KEY` or `FLOO_*` platform vars in app code | Platform credential leak |
| `verify=False` or `rejectUnauthorized: false` on HTTPS | TLS verification disabled |
| Catching exceptions and returning `str(e)` to clients | Internal error exposure |
