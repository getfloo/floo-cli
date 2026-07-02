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

/// Response from creating an org invite. `invite_url` is a one-time link and is
/// treated as secret-shaped by the redactor (see `redact::SECRET_FIELD_NAMES`),
/// so it is redacted in `--json` output unless `--reveal-secrets` is passed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInviteResponse {
    pub email: String,
    pub role: String,
    pub status: String,
    pub invite_url: String,
    pub expires_at: String,
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
    /// Direct Cloud Run runtime URL of the primary user-facing service.
    /// Debug-only — clients should hit `url` (the gateway URL).
    #[serde(default)]
    pub runtime_url: Option<String>,
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
pub struct FailureRootCause {
    pub stage: Option<String>,
    pub reason: Option<String>,
    #[serde(default)]
    pub details: Option<serde_json::Value>,
    #[serde(default)]
    pub first_failure_at: Option<String>,
    #[serde(default)]
    pub root_cause_event_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deploy {
    pub id: String,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub environment_id: Option<String>,
    pub status: Option<String>,
    pub url: Option<String>,
    pub build_logs: Option<String>,
    pub runtime: Option<String>,
    pub created_at: Option<String>,
    pub generated_password: Option<String>,
    pub triggered_by: Option<String>,
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub github_ref: Option<String>,
    #[serde(default)]
    pub environment_name: Option<String>,
    #[serde(default)]
    pub preview_slug: Option<String>,
    #[serde(default)]
    pub source_branch: Option<String>,
    #[serde(default)]
    pub failure_category: Option<String>,
    #[serde(default)]
    pub failure_message: Option<String>,
    #[serde(default)]
    pub failure_step: Option<String>,
    #[serde(default)]
    pub failure_root_cause: Option<FailureRootCause>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub failure_stage: Option<String>,
    #[serde(default)]
    pub failing_stage: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPasswordResponse {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDeploysResponse {
    pub deploys: Vec<Deploy>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub per_page: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
}

// --- Env Var ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: Option<String>,
    pub masked_value: Option<String>,
    // Present on `env list` rows (the API serializes `service_id` per var); used
    // to label which service each var belongs to in the all-services read view.
    // `None` = app-level var. Absent on `env get` responses (defaults to None).
    #[serde(default)]
    pub service_id: Option<String>,
    // Write-only marker (#200 / getfloo/floo#1018): the API never returns this
    // row's value in plaintext. Absent on `env get` responses (a write-only row
    // refuses `get` with ENV_VAR_WRITE_ONLY before a body is ever built).
    #[serde(default)]
    pub is_secret: bool,
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
    #[serde(default)]
    pub is_secret: bool,
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

// --- Managed services ---
// Mirrors api/app/schemas/managed_services.py. Keep these in lock-step.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedServiceSummary {
    pub id: String,
    pub app_id: String,
    #[serde(rename = "type")]
    pub service_type: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub env_var_keys: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListManagedServicesResponse {
    pub managed_services: Vec<ManagedServiceSummary>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateManagedServiceRequest<'a> {
    #[serde(rename = "type")]
    pub service_type: &'a str,
    pub name: &'a str,
    pub tier: &'a str,
}

/// Detail response. Deliberately skips `credentials` — the CLI must never print
/// plaintext secrets. If a future command needs them (e.g. `floo env sync`),
/// add a separate deserialization path rather than exposing them here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedServiceDetail {
    pub id: String,
    pub app_id: String,
    #[serde(rename = "type")]
    pub service_type: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub env_var_keys: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageObjectVersion {
    pub object_path: String,
    pub generation: String,
    pub is_live: bool,
    pub size_bytes: u64,
    pub size_human: String,
    pub updated_at: Option<String>,
    pub created_at: Option<String>,
    pub content_type: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageObjectVersionsResponse {
    pub bucket_name: String,
    pub object_path: String,
    pub versions: Vec<StorageObjectVersion>,
    pub total_returned: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageObjectRestoreResponse {
    pub bucket_name: String,
    pub object_path: String,
    pub restored_generation: String,
    pub live_generation: String,
    pub size_bytes: u64,
    pub size_human: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedPostgresBackup {
    pub id: String,
    pub app_id: String,
    pub managed_service_id: String,
    pub env: String,
    pub status: String,
    pub size_bytes: u64,
    pub size_human: String,
    pub checksum_sha256: String,
    pub created_at: String,
    pub expires_at: String,
    pub last_restored_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedPostgresBackupsResponse {
    pub backups: Vec<ManagedPostgresBackup>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedPostgresRestoreResponse {
    pub backup: ManagedPostgresBackup,
    pub restored_at: String,
}

// --- Preview database branches ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewDatabaseBranch {
    pub id: Option<String>,
    pub managed_service_id: Option<String>,
    pub name: String,
    pub source_environment: String,
    pub preview_slug: String,
    pub resource_status: String,
    pub hydration_mode: String,
    pub schema_name: Option<String>,
    pub role_name: Option<String>,
    pub base_schema_name: Option<String>,
    pub base_role_name: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub expires_at: Option<String>,
    pub reset_eligible: bool,
    pub reset_blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewDatabaseBranchListResponse {
    pub database_branches: Vec<PreviewDatabaseBranch>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewManagedResourceBranch {
    pub id: String,
    pub managed_service_id: Option<String>,
    pub resource_type: String,
    pub name: String,
    pub resource_key: String,
    pub source_environment: String,
    pub preview_slug: String,
    pub resource_status: String,
    pub hydration_mode: String,
    pub schema_name: Option<String>,
    pub role_name: Option<String>,
    pub base_schema_name: Option<String>,
    pub base_role_name: Option<String>,
    pub database_id: Option<String>,
    pub bucket_name: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub expires_at: Option<String>,
    pub reset_eligible: bool,
    pub reset_blocked_reason: Option<String>,
    #[serde(default)]
    pub dev_prod_untouched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewManagedResourceBranchListResponse {
    pub resources: Vec<PreviewManagedResourceBranch>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewResource {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub name: String,
    pub status: String,
    pub external_resource_id: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEnvironment {
    pub id: String,
    pub app_id: String,
    pub slug: String,
    pub source_branch: Option<String>,
    pub pr_number: Option<u32>,
    pub url: Option<String>,
    pub latest_deploy_id: Option<String>,
    pub latest_deploy_status: Option<String>,
    pub latest_commit_sha: Option<String>,
    pub ttl_hours: Option<u32>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub resources: Vec<PreviewResource>,
    #[serde(default)]
    pub database_branches: Vec<PreviewDatabaseBranch>,
    #[serde(default)]
    pub managed_resource_branches: Vec<PreviewManagedResourceBranch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEnvironmentListResponse {
    pub previews: Vec<PreviewEnvironment>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatePreviewDeployRequest<'a> {
    pub runtime: &'a str,
    pub environment: &'a str,
    pub branch: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<&'a str>,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<&'a str>,
}

// --- Preflight ---
// Mirrors api/app/schemas/preflight.py. Keep these two in lock-step.

#[derive(Debug, Clone, Serialize)]
pub struct DeclaredManagedService {
    #[serde(rename = "type")]
    pub service_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DeclaredState {
    pub managed_services: Vec<DeclaredManagedService>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManagedServicePlanItem {
    #[serde(rename = "type")]
    pub service_type: String,
    pub name: String,
    pub tier: Option<String>,
    pub managed_service_id: Option<String>,
    pub data_impact: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ManagedServicesPlan {
    #[serde(default)]
    pub to_provision: Vec<ManagedServicePlanItem>,
    #[serde(default)]
    pub to_retain: Vec<ManagedServicePlanItem>,
    #[serde(default)]
    pub to_orphan: Vec<ManagedServicePlanItem>,
    #[serde(default)]
    pub in_flight_deprovisioning: Vec<ManagedServicePlanItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PlanSummary {
    #[serde(default)]
    pub action_count: u32,
    #[serde(default)]
    pub destructive_count: u32,
    pub estimated_duration_seconds: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PreflightPlan {
    #[serde(default)]
    pub managed_services: ManagedServicesPlan,
    #[serde(default)]
    pub summary: PlanSummary,
    #[serde(default)]
    pub destructive: bool,
    #[serde(default)]
    pub data_loss: bool,
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
    pub deployment_id: Option<String>,
    pub request_id: Option<String>,
    pub labels: Option<serde_json::Value>,
    pub service_name: Option<String>,
    pub cron_job_name: Option<String>,
    pub deploy_context: Option<serde_json::Value>,
    pub severity_class: Option<String>,
    pub lifecycle_noise: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub logs: Vec<LogEntry>,
    pub total: Option<i32>,
    pub app_name: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
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
    /// Keys withheld from the env maps because their rows are write-only
    /// (the API also injects FLOO_WITHHELD_SECRET_KEYS into affected maps).
    #[serde(default)]
    pub withheld_secret_keys: Vec<String>,
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
    pub name: String,
    pub triggered: bool,
    pub message: Option<String>,
}

// --- Doctor (`floo doctor accounts`) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsDoctorRoute {
    pub host: String,
    pub path_prefix: String,
    pub backend_url: String,
    pub serving_access_mode: String,
    pub expected_access_mode: String,
    pub floo_endpoints_wired: bool,
    pub identity_headers_injected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsDoctorRequested {
    pub access_mode: String,
    pub access_policy: String,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsDoctorLatestDeploy {
    pub id: String,
    pub status: String,
    pub requested_app_access_mode: Option<String>,
    pub propagated: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsDoctorDrift {
    /// Drift kind from the API. `String` (not enum) so a CLI built against
    /// an older schema doesn't refuse to render new drift kinds — the API
    /// owns the closed set; the CLI tolerates additions.
    pub kind: String,
    pub summary: String,
    pub likely_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsDoctorResponse {
    pub app_id: String,
    pub app_name: String,
    pub requested: AccountsDoctorRequested,
    pub serving: Vec<AccountsDoctorRoute>,
    pub latest_deploy: Option<AccountsDoctorLatestDeploy>,
    /// Single health verdict (#1156). `Option` + `#[serde(default)]` so a
    /// response from an API predating this field deserializes as `None` instead
    /// of erroring. The command does not trust the incoming value: it derives
    /// the verdict from `drift` (the evidence it renders) and canonicalizes it
    /// back onto this field, so the emitted body always carries a definitive
    /// bool that agrees with both the drift list and the exit code.
    #[serde(default)]
    pub drift_detected: Option<bool>,
    pub drift: Vec<AccountsDoctorDrift>,
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
    // Requests the gateway answered without proxying to the backend (rejections
    // + gateway-owned endpoints). Explains why latency is measured over fewer
    // requests than total. `#[serde(default)]` keeps the CLI working against an
    // API that predates the field (None = unknown).
    #[serde(default)]
    pub gateway_handled_requests: Option<i64>,
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

// --- Notification preferences ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreference {
    pub category: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    // True while inheriting the system default; false once the user has chosen.
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferencesResponse {
    pub preferences: Vec<NotificationPreference>,
}
