floo - Build with your stack

Stack-specific journey guides walk a real app from local code to a live
production URL with a database, per-user auth, and a custom domain. Each
guide is end-to-end with runnable code in your stack.

## Available stack guides

  floo docs nextjs   - build and deploy a Next.js (App Router) app on floo
  floo docs rails    - build and deploy a Ruby on Rails app on floo
  floo docs fastapi  - build and deploy a FastAPI app on floo
  floo docs django   - build and deploy a Django app on floo
  floo docs express  - build and deploy an Express (Node.js) app on floo

  Want a stack added (Go, SvelteKit, Phoenix, etc.)?
  `floo feedback --category feature_request "docs: add <stack> stack guide"`

## What a stack guide covers

Every stack guide walks the same arc, with stack-specific code:

  1. Add a Dockerfile (or use the framework's default)
  2. Initialize floo config (floo init)
  3. Connect the GitHub repo and ship the first deploy
  4. Add a Postgres sibling service
  5. Add per-user auth (gateway-managed - zero app code)
  6. Add a custom domain
  7. Run locally with prod credentials

## Why stack guides vs reference docs

Capability guides (floo docs auth, floo docs services, floo docs config)
explain how floo features work. Stack guides show how to use them
end-to-end in a real Rails / Next.js / Django / etc. project.

If you know which capability you need, jump to the capability guide. If
you're starting a new project, start with the stack guide.

Full guides: https://getfloo.com/docs/build/
