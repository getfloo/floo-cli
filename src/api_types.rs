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

// --- Org ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgResponse {
    pub id: String,
    pub name: Option<String>,
    pub spend_cap: Option<u64>,
    pub current_period_spend_cents: Option<u64>,
    pub spend_cap_exceeded: Option<bool>,
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
