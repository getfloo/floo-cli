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
pub struct BillingCheckoutResponse {
    pub url: String,
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
pub struct App {
    pub id: String,
    pub name: String,
    pub org_id: Option<String>,
    pub status: Option<String>,
    pub url: Option<String>,
    pub runtime: Option<String>,
    pub created_at: Option<String>,
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
    pub key: Option<String>,
    pub masked_value: Option<String>,
    pub status: Option<String>,
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

// --- Images ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseImage {
    pub name: String,
    pub tag: String,
    pub public_uri: String,
    pub mirror_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseImagesResponse {
    pub images: Vec<BaseImage>,
}

// --- Dev Session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevSessionService {
    pub name: String,
    pub port: u16,
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
    pub summary: AnalyticsSummary,
    pub time_series: Option<Vec<TimeSeriesPoint>>,
    pub apps: Option<Vec<AppAnalyticsEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub request_count: i64,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppAnalyticsEntry {
    pub app_name: String,
    pub total_requests: i64,
    pub total_errors: i64,
    pub error_rate: f64,
}
