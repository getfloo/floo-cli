floo - Build a Next.js app on floo

End-to-end Next.js 14+ App Router journey: deploy, add Postgres, add
per-user auth, add a custom domain. Every step has runnable TypeScript
code in the published guide.

## 1. Dockerfile (standalone)

Set output: "standalone" in next.config.js, then a multi-stage Dockerfile
ending with `CMD ["node", "server.js"]`. Set HOSTNAME=0.0.0.0 (Next.js
standalone defaults to localhost - Cloud Run won't reach it).

## 2. NEXT_PUBLIC_* build-arg trap

Any NEXT_PUBLIC_* var is baked into the JS bundle at BUILD TIME. Thread
it through the Dockerfile as ARG + ENV in the build stage AND pass it
on every build:

  floo env set NEXT_PUBLIC_API_URL=https://my-app.on.getfloo.com --app my-app

Skipping this is the most common Next.js footgun on floo.

## 3. floo init + deploy

  floo init my-nextjs-app

  [services.web]
  type = "web"
  path = "."
  port = 3000
  ingress = "public"
  dev_command = "npm run dev"
  migrate_command = "npx prisma migrate deploy"   # if you use Prisma

  git push origin main
  floo apps github connect owner/my-nextjs-app

## 4. Postgres

  [managed.primary]
  type = "postgres"
  tier = "basic"

  # Prisma reads DATABASE_URL automatically

## 5. Per-user auth

  [app]
  access_mode = "accounts"

Then in a Server Component or Route Handler:

  import { headers } from "next/headers";
  const h = await headers();
  const email = h.get("x-floo-user-email");

## 6. Custom domain

  floo domains add app.example.com --app my-nextjs-app

## 7. Local dev

  floo dev --app my-nextjs-app

## Gotchas

  - /healthz is reserved by Cloud Run - use /health
  - HOSTNAME=0.0.0.0 (standalone defaults to localhost)
  - NEXT_PUBLIC_* changes require floo redeploy --rebuild
  - output: "standalone" in next.config.js
  - Never expose tokens to client components

Full guide with complete TypeScript code: https://getfloo.com/docs/build/nextjs
