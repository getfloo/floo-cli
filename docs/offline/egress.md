floo Egress and Networking

floo puts your app behind the floo gateway for inbound traffic. Your app's
outbound traffic uses floo's normal internet egress today.

## Inbound traffic

  client
    -> *.on.getfloo.com or custom domain
    -> floo gateway
    -> edge policy and managed auth
    -> your app service

Do not expose or depend on raw backend URLs. Treat floo hosts and
custom domains as the public contract.

Inspect the live route table with:

  floo edge routes list --app my-app --env prod
  floo edge routes list --app my-app --env prod --json

## Outbound egress

Current contract:

  Public HTTPS APIs             supported
  Slack/payment/webhook calls   supported through normal SDKs or HTTP clients
  Stable outbound source IP     not provided today
  SMTP port 25                  not available
  SMTP submission               use 587 or 465 with provider auth
  Private VPC or tailnet apps   not generally available today

If a vendor requires source-IP allowlisting, do not assume floo has a fixed
egress IP for your app. Prefer API-token auth, OAuth, mutual TLS, or the
vendor's HTTPS API. If fixed egress is mandatory, email team@getfloo.com before
designing around it.

## Private network deployments

Private network deployments are planned as an enterprise connector model, not a
current config toggle:

  - floo control plane stays public
  - app traffic to private backends stays inside the customer network
  - a customer-side connector opens an outbound-only tunnel to floo
  - floo gateway remains the auth, edge-policy, and audit boundary
  - tailnet is one possible transport, not the whole product model

The connector lifecycle, routing model, auth model, and tunnel-down behavior
will be documented before this is enabled for production workloads.

Full guide: https://getfloo.com/docs/guides/networking
