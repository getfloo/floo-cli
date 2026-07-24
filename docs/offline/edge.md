floo Edge Routes

Inspect the route table floo's edge is serving for an app. Use this when a
host, path, target service, access mode, or custom domain does not behave the
way you expected.

## List routes

  floo edge routes list --app my-app
  floo edge routes list --app my-app --env prod
  floo edge routes list --app my-app --env preview --json

The route table shows:

  host              Customer-facing floo host or custom domain
  path_prefix       Path prefix matched by the gateway
  environment       dev, prod, preview, or unscoped legacy route
  service           Target app service name and type
  policy            Effective access mode and app API-key requirement
  source            deploy, custom_domain, toml, or system
  source_of_truth   gateway_routes

JSON output is stable for agents:

  floo edge routes list --app my-app --env prod --json 2>/dev/null |
    jq '.data.routes[] | {host, path_prefix, access_mode, api_key_enabled, required_scope}'

The output deliberately omits raw Cloud Run backend URLs. Treat floo hosts and
custom domains as the public contract.

## Edge policy (IP/CIDR firewall, Team plan)

An ordered allow/deny rule list per app + environment, enforced at floo's
edge BEFORE the request body is read and before any managed auth. Rules are
evaluated top to bottom; first match wins; the default action applies when
no rule matches. Previews inherit the dev policy.

The firewall is configured as code in floo.app.toml and applied on deploy -
that is the single source of truth (version-controlled, reviewable, auditable).
The CLI is read-only.

  [edge]                       # office-only allowlist: deny everything else
  default_action = "deny"

  [[edge.rules]]
  action = "allow"
  cidr = "203.0.113.0/24"

  [environments.prod.edge]     # per-env override (previews inherit dev)

Change the firewall by editing floo.app.toml and deploying. Read it with:

  floo edge policy get --env prod --json
  floo edge policy check 203.0.113.7 --env prod   # would this IP be admitted?

Denied requests get 403 {"code":"EDGE_POLICY_DENIED"}; denial counts appear
in `floo analytics` as the rejection breakdown.

## Enforcement order

Requests pass gates in this order - an earlier denial short-circuits:

  1. Cloud provider edge (TLS, volumetric DDoS)   [floo-managed]
  2. Edge policy (this firewall)                  [yours]
  3. Managed auth (access_mode, app API keys)     [yours]
  4. Your app

The edge policy cannot see or bypass auth; auth never runs for a denied IP.

Full reference: https://getfloo.com/docs/cli/edge
