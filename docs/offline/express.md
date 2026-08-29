floo - Build an Express app on floo

End-to-end Express 4/5 journey: deploy, add Postgres, add per-user
auth, add a custom domain. Every step has runnable JavaScript code
in the published guide.

## 1. Dockerfile

  FROM node:20-slim AS deps
  ...
  CMD ["node", "server.js"]

## 2. Trust the proxy

  app.set("trust proxy", true);
  app.listen(port, "0.0.0.0", ...);

Without trust proxy, req.protocol is always 'http' and secure cookies
won't get set behind floo's edge.

## 3. floo init + deploy

  floo init my-express-app

  [services.web]
  type = "web"
  path = "."
  port = 3000
  ingress = "public"
  dev_command = "node --watch server.js"

  git push origin main
  floo apps github connect owner/my-express-app

## 4. Postgres

  [managed.primary]
  type = "postgres"
  tier = "basic"

  import pg from "pg";
  export const pool = new pg.Pool({
    connectionString: process.env.DATABASE_URL, max: 10
  });

## 5. Per-user auth

  [app]
  access_mode = "accounts"

  app.use((req, _res, next) => {
    const email = req.get("x-floo-user-email");
    req.flooUser = email ? { email, id: req.get("x-floo-user-id") } : null;
    next();
  });

## 6. Custom domain

  floo domains add app.example.com --app my-express-app

## 7. Local dev

  floo dev --app my-express-app

## Gotchas

  - /healthz is reserved by floo - use /health
  - app.listen(port, "0.0.0.0", ...) explicitly
  - app.set("trust proxy", true) is required
  - SESSION_SECRET required for cookie-session
  - For server-side sessions: declare a managed Redis handle and use connect-redis

Full guide with complete JavaScript code: https://getfloo.com/docs/build/express
