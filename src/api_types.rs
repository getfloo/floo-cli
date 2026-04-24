use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// --- Auth ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoamiResponse {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthorizeResponse {
    pub user_code: String,
    pub verification_uri_complete: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTokenResponse {
    pub api_key: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileResponse {
    pub name: String,
}

// --- Billing ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppCostSummary {
    pub app_id: String,
    pub name: String,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdownPeriod {
    pub start: String,
    pub end: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgCostBreakdownResponse {
    pub period: CostBreakdownPeriod,
    pub total_cost_usd: f64,
    pub included_cost_usd: f64,
    pub apps: Vec<AppCostSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingCheckoutResponse {
    pub url: Option<String>,
    #[serde(default)]
    pub upgraded: bool,
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanLimitsResponse {
    pub plan: String,
    pub max_spend_cap_cents: Option<u64>,
}

// --- Org ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgResponse {
    pub id: String,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub plan: Option<String>,
    pub spend_cap: Option<u64>,
    pub current_period_spend_cents: Option<u64>,
    pub spend_cap_exceeded: Option<bool>,
}

impl OrgResponse {
    /// Human-readable display name: prefers slug, falls back to name.
    pub fn display_name(&self) -> Option<&str> {
        self.slug.as_deref().or(self.name.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOrgsResponse {
    pub orgs: Vec<OrgResponse>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMembersResponse {
    pub members: Vec<OrgMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgMember {
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberRoleResponse {
    pub email: String,
    pub role: String,
}

// --- App ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSummary {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub org_id: Option<String>,
    pub status: Option<String>,
    pub url: Option<String>,
    pub runtime: Option<String>,
    pub created_at: Option<String>,
    #[serde(default)]
    pub environments: Vec<EnvironmentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAppsResponse {
    pub apps: Vec<App>,
    pub total: Option<u64>,
}

// --- Deploy ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deploy {
    pub id: String,
    pub status: Option<String>,
    pub url: Option<String>,
    pub build_logs: Option<String>,
    pub runtime: Option<String>,
    pub created_at: Option<String>,
    pub generated_password: Option<String>,
    pub triggered_by: Option<String>,
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub environment_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPasswordResponse {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDeploysResponse {
    pub deploys: Vec<Deploy>,
}

// --- Env Var ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: Option<String>,
    pub masked_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListEnvVarsResponse {
    pub env_vars: Vec<EnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetEnvVarResponse {
    pub id: String,
    pub app_id: String,
    pub environment_id: String,
    pub service_id: Option<String>,
    pub key: String,
    pub masked_value: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// --- Service ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiService {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub service_type: Option<String>,
    pub status: Option<String>,
    pub ingress: Option<String>,
    pub cloud_run_url: Option<String>,
    pub port: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListServicesResponse {
    pub services: Vec<ApiService>,
}

// --- Domain ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    pub hostname: String,
    pub status: Option<String>,
    pub dns_instructions: Option<String>,
    pub service_name: Option<String>,
    pub ssl_status: Option<String>,
    pub verified: Option<bool>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDomainsResponse {
    pub domains: Vec<Domain>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDomainResponse {
    pub hostname: Option<String>,
    pub status: Option<String>,
    pub dns_instructions: Option<String>,
}

// --- Logs ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: Option<String>,
    pub severity: Option<String>,
    pub message: Option<String>,
    pub service_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogEntry {
    pub timestamp: String,
    pub method: String,
    pub path: Option<String>,
    pub host: Option<String>,
    pub status_code: i32,
    pub latency_ms: i32,
    pub access_mode: String,
    pub user_identity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogsResponse {
    pub requests: Vec<RequestLogEntry>,
    pub total: i32,
    pub app_name: String,
}

// --- Release ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub release_number: Option<u64>,
    pub tag: Option<String>,
    pub commit_sha: Option<String>,
    pub promoted_by: Option<String>,
    pub created_at: Option<String>,
    pub deploy_id: Option<String>,
    pub image_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListReleasesResponse {
    pub releases: Vec<Release>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteResponse {
    pub tag: String,
    pub release_url: String,
}

// --- GitHub ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConnectResponse {
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubStatusResponse {
    pub id: String,
    pub app_id: String,
    pub skip_env_var_check: bool,
    pub preview_enabled: bool,
    pub preview_ttl_hours: Option<i64>,
    pub connected_at: String,
    pub repo_full_name: Option<String>,
    pub default_branch: Option<String>,
    pub installation_id: Option<i64>,
    pub services: Vec<ServiceGitHubInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceGitHubInfo {
    pub name: String,
    pub repo_full_name: Option<String>,
    pub default_branch: Option<String>,
    pub installation_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitHubSetupStatus {
    None,
    AwaitingInstallation,
    AwaitingOrgApproval,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubSetupPollResponse {
    pub status: GitHubSetupStatus,
    pub installation_id: Option<i64>,
}

// --- Database ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseInfo {
    pub host: String,
    pub port: u64,
    pub database: String,
    pub name: Option<String>,
    pub status: Option<String>,
    pub username: Option<String>,
    pub schema_name: Option<String>,
}

// --- Dev Session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevSessionService {
    pub name: String,
    /// Port is required for `floo dev` (used for cross-service discovery vars).
    /// Omit (None) for `floo run` one-shot mode — the API skips discovery in that case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevSessionResponse {
    pub session_id: String,
    pub services: HashMap<String, HashMap<String, String>>,
    pub postgres_authorized: bool,
}

// --- Cron Jobs ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobResponse {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub command: String,
    pub service_name: String,
    pub timeout: u32,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub last_status: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobListResponse {
    pub cron_jobs: Vec<CronJobResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobRunResponse {
    pub status: String,
    pub message: Option<String>,
}

// --- Analytics ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub total_requests: i64,
    pub total_errors: i64,
    pub error_rate: f64,
    pub avg_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub unique_users: Option<i64>,
    pub status_code_breakdown: Option<HashMap<String, i64>>,
    pub total_apps_with_traffic: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppAnalyticsResponse {
    pub app_id: Option<String>,
    pub period: Option<String>,
    pub summary: AnalyticsSummary,
    #[serde(default)]
    pub time_series: Vec<TimeSeriesPoint>,
    #[serde(default)]
    pub top_users: Vec<TopUser>,
    // org-level response reuses this struct and supplies `apps`; for app-level
    // it stays an empty array. `#[serde(default)]` keeps it [] in JSON output
    // instead of null.
    #[serde(default)]
    pub apps: Vec<AppAnalyticsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub period_start: String,
    pub request_count: i64,
    #[serde(default)]
    pub error_count: i64,
    #[serde(default)]
    pub error_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopUser {
    pub identity: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppAnalyticsEntry {
    pub app_id: Option<String>,
    pub app_name: String,
    pub total_requests: i64,
    pub total_errors: i64,
    pub error_rate: f64,
    pub avg_latency_ms: Option<i64>,
}
