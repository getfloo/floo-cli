floo Preview Sandboxes

Use `floo previews` when an agent needs an isolated, real floo environment for a
pushed feature branch before opening or relying on a pull request preview.

## Source contract

Preview sandboxes deploy remote GitHub source only:

  git push origin feat/foo
  floo previews up --app my-app --branch feat/foo --wait

The CLI does not upload local dirty files or an archive from your checkout.
Push the branch first, or pass a remote commit/ref when you need an exact
remote revision.

## Lifecycle commands

  floo previews up --app my-app --branch feat/foo --wait --json
  floo previews list --app my-app --json
  floo previews status --app my-app feat/foo --json
  floo previews logs --app my-app feat/foo --follow
  floo previews delete --app my-app feat/foo --yes --json

Preview identifiers can be an exact slug, a preview URL, the source branch,
or `#123` when that PR number resolves to one preview. If the identifier is
ambiguous, use the exact slug from `floo previews list`.

## JSON contract

Non-streaming commands print one JSON object. Automation can rely on:

  app
  preview.slug
  source_branch
  deploy_id
  status
  url
  expires_at
  database_branches
  managed_resource_branches
  dev_prod_untouched: true

`up --wait` watches the deploy returned by the create call and exits non-zero
when that deploy fails.

## Isolation and cleanup

Preview sandboxes use the same managed-resource isolation as pull request
previews. floo-managed Postgres, Redis, and Storage get preview-owned
resources. If isolation cannot be provisioned, the command surfaces
PREVIEW_MANAGED_SERVICE_ISOLATION_UNAVAILABLE instead of falling back to dev
or prod credentials.

Preview managed-resource branches are visible through one command group:

  floo previews resources list <preview> --app <name>
  floo previews resources show <preview> --app <name> --resource redis:default
  floo previews resources reset <preview> --app <name> --resource postgres:default --yes

The resource key is shaped `type:name`, for example `postgres:default`,
`redis:cache`, or `storage:uploads`. Reset is preview-scoped and fails closed
with the API's named blocker when a provider cannot reset that resource yet.

`floo previews delete` tears down preview-owned services, managed
resources, gateway routes, and env vars. Dev and prod are untouched.

## Related commands

  floo db branches list <preview> --app <name>
  floo db branches show <preview> --app <name> --name default
  floo db branches reset <preview> --app <name> --name default --yes

Full guide: https://getfloo.com/docs/cli/previews
