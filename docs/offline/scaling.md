floo Scaling and Availability

Use `floo.app.toml` for every runtime scaling choice. There is no separate
dashboard or CLI write path and CPU allocation is not customer-configurable.

## Deterministic selection rules

Choose on-demand for an HTTP service when the first request after an idle
period may cold-start and the lowest idle cost matters most:

  [services.web]
  type = "web"
  path = "."
  port = 3000
  min_instances = 0
  max_instances = 3

Choose warm only when a user-facing latency requirement justifies a continuous
idle cost floor:

  [services.web]
  type = "web"
  path = "."
  port = 3000
  min_instances = 1
  max_instances = 3

A warm HTTP service keeps one baseline instance ready and can still autoscale
up to `max_instances`. The baseline avoids its cold start; new burst capacity
may still cold-start. Warm does not mean CPU always on: web and api services use
request-based CPU, including when attached to floo-managed Postgres.

Use a worker for work that must run without an HTTP request. Workers have a
fixed count and always-allocated CPU:

  [services.worker]
  type = "worker"
  path = "."
  port = 3000
  command = "bundle exec sidekiq"
  instances = 1

Pause a worker explicitly with `instances = 0`. Workers never use
`min_instances`; global `[resources] min_instances` applies only to web/api.

## Defaults, limits, and verification

- HTTP `min_instances` defaults to 0 in dev and in the locally resolved plan.
- HTTP `max_instances` defaults to 3.
- Worker `instances` defaults to 1.
- Per-service values override delegated `floo.service.toml` values, which
  override global `[resources]` values.
- The server applies plan and platform ceilings. Local preflight marks those
  values as requiring server resolution instead of pretending a clamp is known.

Run `floo preflight --json` before pushing. Read `data.runtime_plan` for the
configured values, locally resolved defaults, field sources, availability
posture, and CPU mode. After deployment, use
`floo services show <name> --app <app> --json` to verify the server-effective
runtime plan.
