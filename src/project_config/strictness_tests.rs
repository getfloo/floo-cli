use super::{load_app_config, load_service_config, APP_CONFIG_FILE, SERVICE_CONFIG_FILE};
use crate::errors::ErrorCode;

macro_rules! rejects_app_key {
    ($name:ident, $manifest:literal, $path:literal) => {
        #[test]
        fn $name() {
            let _guard = crate::output::GLOBAL_MODE_LOCK.lock().unwrap();
            crate::output::set_json_mode(false);
            crate::output::set_dry_run_mode(false);
            let dir = tempfile::TempDir::new().unwrap();
            std::fs::write(
                dir.path().join(APP_CONFIG_FILE),
                concat!($manifest, "\n[app]\nname = 'my-app'\n"),
            )
            .unwrap();

            let err = load_app_config(dir.path()).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
            let suggestion = err.suggestion.unwrap();
            assert!(
                suggestion.contains(concat!("`", $path, "`")),
                "{suggestion}"
            );
            assert!(!suggestion.contains("floo update"), "{suggestion}");
        }
    };
}

rejects_app_key!(rejects_misspelled_cron_table, "[crons.backup]", "crons");
rejects_app_key!(rejects_root_env_table, "[env]\nTOKEN = 'value'", "env");
rejects_app_key!(
    rejects_domain_typo,
    "[domains.\"app.example.com\"]\nservce = 'web'",
    "domains.app.example.com.servce"
);
rejects_app_key!(rejects_root_scalar, "timeuot = 900", "timeuot");
rejects_app_key!(
    rejects_removed_reparo,
    "[reparo]\nmode = 'webhook'",
    "reparo"
);
rejects_app_key!(
    rejects_cron_timeout_typo,
    "[cron.backup]\nschedule = '* * * * *'\ncommand = 'backup'\nservice = 'web'\ntimeuot = 900",
    "cron.backup.timeuot"
);
rejects_app_key!(
    rejects_github_typo,
    "[github]\npreview_ttl_hour = 2",
    "github.preview_ttl_hour"
);
rejects_app_key!(
    rejects_auth_typo,
    "[auth]\naccess_polciy = 'open'",
    "auth.access_polciy"
);
rejects_app_key!(
    rejects_postgres_typo,
    "[postgres]\ntire = 'small'",
    "postgres.tire"
);
rejects_app_key!(rejects_redis_typo, "[redis]\ntire = 'small'", "redis.tire");
rejects_app_key!(
    rejects_storage_typo,
    "[storage]\ntire = 'small'",
    "storage.tire"
);
rejects_app_key!(
    rejects_managed_typo,
    "[managed.db]\ntype = 'postgres'\ntire = 'small'",
    "managed.db.tire"
);
rejects_app_key!(
    rejects_environment_typo,
    "[environments.staging]\naccess_mod = 'public'",
    "environments.staging.access_mod"
);
rejects_app_key!(
    rejects_edge_typo,
    "[edge]\ndefault_acton = 'deny'",
    "edge.default_acton"
);
rejects_app_key!(
    rejects_edge_rule_typo,
    "[[edge.rules]]\naction = 'deny'\ncdir = '10.0.0.0/8'",
    "edge.rules.cdir"
);
rejects_app_key!(
    rejects_resource_typo,
    "[resources]\nmax_instance = 2",
    "resources.max_instance"
);
rejects_app_key!(
    rejects_service_typo,
    "[services.web]\ntype = 'web'\nprot = 3000",
    "services.web.prot"
);
rejects_app_key!(
    rejects_env_contract_typo,
    "[services.web]\ntype = 'web'\n[services.web.env]\nrequried = ['KEY']",
    "services.web.env.requried"
);
rejects_app_key!(
    rejects_dotted_key_typo,
    "services.web = { type = 'web', prot = 3000 }",
    "services.web.prot"
);
rejects_app_key!(
    rejects_removed_version,
    "[services.web]\ntype = 'web'\nversion = '1'",
    "services.web.version"
);
rejects_app_key!(
    rejects_removed_plan,
    "[services.web]\ntype = 'web'\nplan = 'small'",
    "services.web.plan"
);
rejects_app_key!(
    rejects_removed_allowed_domains,
    "[auth]\nallowed_domains = ['example.com']",
    "auth.allowed_domains"
);

#[test]
fn rejects_removed_agent_mode() {
    let _guard = crate::output::GLOBAL_MODE_LOCK.lock().unwrap();
    crate::output::set_json_mode(false);
    crate::output::set_dry_run_mode(false);
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join(APP_CONFIG_FILE),
        "[app]\nname = 'my-app'\nagent_mode = true",
    )
    .unwrap();
    let err = load_app_config(dir.path()).unwrap_err();
    assert!(err.suggestion.unwrap().contains("`app.agent_mode`"));
}

#[test]
fn delegated_service_errors_name_the_key_path() {
    let _guard = crate::output::GLOBAL_MODE_LOCK.lock().unwrap();
    crate::output::set_json_mode(false);
    crate::output::set_dry_run_mode(false);
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join(SERVICE_CONFIG_FILE),
        "[app]\nname = 'my-app'\n[service]\nname = 'web'\ntype = 'web'\nport = 3000\n[env]\nrequried = ['KEY']",
    )
    .unwrap();
    let err = load_service_config(dir.path()).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidProjectConfig);
    let suggestion = err.suggestion.unwrap();
    assert!(suggestion.contains("`env.requried`"), "{suggestion}");
    assert!(!suggestion.contains("floo update"), "{suggestion}");
}

#[test]
fn accepts_supported_cron_fields() {
    let _guard = crate::output::GLOBAL_MODE_LOCK.lock().unwrap();
    crate::output::set_json_mode(false);
    crate::output::set_dry_run_mode(false);
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join(APP_CONFIG_FILE),
        "[app]\nname = 'my-app'\n[cron.backup]\nschedule = '* * * * *'\ncommand = 'backup'\nservice = 'web'\ntimeout = 900",
    )
    .unwrap();
    let config = load_app_config(dir.path()).unwrap().unwrap();
    assert_eq!(config.cron["backup"].timeout, Some(900));
}
