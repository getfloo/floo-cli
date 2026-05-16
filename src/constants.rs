pub const DEFAULT_API_URL: &str = "https://api.getfloo.com";
// Dev-stack API. The `floo-dev` binary defaults here instead of prod
// (still overridable via FLOO_API_URL). Sourced from
// .github/workflows/dev.yml (API_BASE) and
// scripts/ops/reconcile-gateway-dev-env.sh in the floo monorepo.
pub const DEV_API_URL: &str = "https://api.dev.getfloo.com";
pub const CONFIG_FILE_NAME: &str = "config.json";
pub const VERSION: &str = env!("FLOO_VERSION");
