use crate::{constants::VERSION, output};

const DOCS_SCHEMA_VERSION: u32 = 1;
#[cfg(test)]
const MAX_EMBEDDED_DOCS_BYTES: usize = 512 * 1024;

const OVERVIEW: &str = include_str!("../../docs/offline/overview.md");

const QUICKSTART: &str = include_str!("../../docs/offline/quickstart.md");

const SERVICES: &str = include_str!("../../docs/offline/services.md");

const DOCTOR: &str = include_str!("../../docs/offline/doctor.md");

const EDGE: &str = include_str!("../../docs/offline/edge.md");

const EGRESS: &str = include_str!("../../docs/offline/egress.md");

const PREVIEWS: &str = include_str!("../../docs/offline/previews.md");

const CONFIG: &str = include_str!("../../docs/offline/config.md");

const SCALING: &str = include_str!("../../docs/offline/scaling.md");

const DEPLOY: &str = include_str!("../../docs/offline/deploy.md");

const AUTH: &str = include_str!("../../docs/offline/auth.md");

const FEEDBACK: &str = include_str!("../../docs/offline/feedback.md");

const NOTIFICATIONS: &str = include_str!("../../docs/offline/notifications.md");

const HOWTO: &str = include_str!("../../docs/offline/golden-path.md");

const TEMPLATES: &str = include_str!("../../docs/offline/templates.md");

const BUILD: &str = include_str!("../../docs/offline/build.md");

const RAILS: &str = include_str!("../../docs/offline/rails.md");

const NEXTJS: &str = include_str!("../../docs/offline/nextjs.md");

const FASTAPI: &str = include_str!("../../docs/offline/fastapi.md");

const DJANGO: &str = include_str!("../../docs/offline/django.md");

const EXPRESS: &str = include_str!("../../docs/offline/express.md");

const CRON: &str = include_str!("../../docs/offline/cron.md");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Topic {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) content: &'static str,
}

const NO_ALIASES: &[&str] = &[];
const SERVICES_ALIASES: &[&str] = &["storage"];
const CONFIG_ALIASES: &[&str] = &["app-toml"];

/// Canonical offline topics in display order. This registry owns discovery,
/// aliases, summaries, and the exact Markdown embedded into the binary.
pub(crate) const TOPICS: &[Topic] = &[
    Topic {
        name: "golden-path",
        summary: "Golden path and decision table",
        aliases: NO_ALIASES,
        content: HOWTO,
    },
    Topic {
        name: "quickstart",
        summary: "End-to-end setup and first deploy",
        aliases: NO_ALIASES,
        content: QUICKSTART,
    },
    Topic {
        name: "build",
        summary: "Stack-specific build journeys",
        aliases: NO_ALIASES,
        content: BUILD,
    },
    Topic {
        name: "nextjs",
        summary: "Build and deploy a Next.js app",
        aliases: NO_ALIASES,
        content: NEXTJS,
    },
    Topic {
        name: "rails",
        summary: "Build and deploy a Rails app",
        aliases: NO_ALIASES,
        content: RAILS,
    },
    Topic {
        name: "fastapi",
        summary: "Build and deploy a FastAPI app",
        aliases: NO_ALIASES,
        content: FASTAPI,
    },
    Topic {
        name: "django",
        summary: "Build and deploy a Django app",
        aliases: NO_ALIASES,
        content: DJANGO,
    },
    Topic {
        name: "express",
        summary: "Build and deploy an Express app",
        aliases: NO_ALIASES,
        content: EXPRESS,
    },
    Topic {
        name: "templates",
        summary: "Copy-paste app structures",
        aliases: NO_ALIASES,
        content: TEMPLATES,
    },
    Topic {
        name: "services",
        summary: "App services and managed services",
        aliases: SERVICES_ALIASES,
        content: SERVICES,
    },
    Topic {
        name: "doctor",
        summary: "Managed-service health and accounts drift",
        aliases: NO_ALIASES,
        content: DOCTOR,
    },
    Topic {
        name: "edge",
        summary: "Routes, access rules, and edge enforcement",
        aliases: NO_ALIASES,
        content: EDGE,
    },
    Topic {
        name: "egress",
        summary: "Outbound networking and private-network status",
        aliases: NO_ALIASES,
        content: EGRESS,
    },
    Topic {
        name: "previews",
        summary: "Remote-branch preview sandboxes",
        aliases: NO_ALIASES,
        content: PREVIEWS,
    },
    Topic {
        name: "config",
        summary: "Configuration file formats and examples",
        aliases: CONFIG_ALIASES,
        content: CONFIG,
    },
    Topic {
        name: "scaling",
        summary: "Availability, scaling, and CPU behavior",
        aliases: NO_ALIASES,
        content: SCALING,
    },
    Topic {
        name: "cron",
        summary: "Scheduled-job schema and operations",
        aliases: NO_ALIASES,
        content: CRON,
    },
    Topic {
        name: "deploy",
        summary: "Git-driven deploy lifecycle and detection",
        aliases: NO_ALIASES,
        content: DEPLOY,
    },
    Topic {
        name: "auth",
        summary: "Authentication for floo and hosted apps",
        aliases: NO_ALIASES,
        content: AUTH,
    },
    Topic {
        name: "notifications",
        summary: "Deployment email preferences",
        aliases: NO_ALIASES,
        content: NOTIFICATIONS,
    },
    Topic {
        name: "feedback",
        summary: "Report product friction from the CLI",
        aliases: NO_ALIASES,
        content: FEEDBACK,
    },
];

pub fn docs(topic: Option<&str>) {
    let Some(requested) = topic else {
        let content = render_overview();
        if output::is_json_mode() {
            output::success(
                "docs:overview",
                Some(serde_json::json!({
                    "schema_version": DOCS_SCHEMA_VERSION,
                    "cli_version": VERSION,
                    "topic": "overview",
                    "content": content,
                    "topics": topic_index_json(),
                })),
            );
        } else {
            eprintln!("{content}");
        }
        return;
    };

    let Some((topic, is_alias)) = resolve_topic(requested) else {
        let available: Vec<&str> = TOPICS.iter().map(|topic| topic.name).collect();
        output::error(
            &format!("Unknown docs topic: '{requested}'."),
            &crate::errors::ErrorCode::InvalidFormat,
            Some(&format!(
                "Available offline topics in floo {VERSION}: {}. Run `floo update` to check for a newer knowledge pack.",
                available.join(", ")
            )),
        );
        std::process::exit(1);
    };

    if !output::is_json_mode() {
        eprintln!("{}", topic.content.trim());
        return;
    }

    let mut data = serde_json::json!({
        "schema_version": DOCS_SCHEMA_VERSION,
        "cli_version": VERSION,
        "topic": topic.name,
        "content": topic.content.trim(),
    });
    if is_alias {
        data["requested_topic"] = serde_json::json!(requested);
        data["alias"] = serde_json::json!(true);
    }
    output::success(&format!("docs:{}", topic.name), Some(data));
}

fn render_overview() -> String {
    let mut rendered = OVERVIEW.trim().to_string();
    rendered.push_str("\n\n## Offline topics\n\n");
    for topic in TOPICS {
        let aliases = if topic.aliases.is_empty() {
            String::new()
        } else {
            format!(" (alias: {})", topic.aliases.join(", "))
        };
        rendered.push_str(&format!(
            "  floo docs {:<14} {}{aliases}\n",
            topic.name, topic.summary
        ));
    }
    rendered.push_str(
        "\n  floo commands --json     machine-readable command catalog\n  floo <command> --help    exact flags, arguments, and examples",
    );
    rendered
}

fn topic_index_json() -> Vec<serde_json::Value> {
    TOPICS
        .iter()
        .map(|topic| {
            serde_json::json!({
                "name": topic.name,
                "summary": topic.summary,
                "aliases": topic.aliases,
            })
        })
        .collect()
}

/// Resolve a canonical topic or convenience alias. The boolean records whether
/// the request used an alias so JSON consumers can self-correct.
fn resolve_topic(requested: &str) -> Option<(&'static Topic, bool)> {
    if let Some(topic) = TOPICS.iter().find(|topic| topic.name == requested) {
        return Some((topic, false));
    }
    TOPICS
        .iter()
        .find(|topic| topic.aliases.contains(&requested))
        .map(|topic| (topic, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docs_content_not_empty() {
        assert!(!OVERVIEW.is_empty());
        assert!(!QUICKSTART.is_empty());
        assert!(!SERVICES.is_empty());
        assert!(!DOCTOR.is_empty());
        assert!(!PREVIEWS.is_empty());
        assert!(!CONFIG.is_empty());
        assert!(!SCALING.is_empty());
        assert!(!CRON.is_empty());
        assert!(!DEPLOY.is_empty());
        assert!(!AUTH.is_empty());
        assert!(!TEMPLATES.is_empty());
        assert!(!BUILD.is_empty());
        assert!(!NEXTJS.is_empty());
        assert!(!RAILS.is_empty());
        assert!(!FASTAPI.is_empty());
        assert!(!DJANGO.is_empty());
        assert!(!EXPRESS.is_empty());
    }

    /// The 2026-04-30 cron-docs feedback: `floo docs app-toml` mentioned cron
    /// as a feature but never showed the [cron.<name>] schema. CONFIG must
    /// document the schema (fields + example) so agents can author cron jobs
    /// without leaving the cheatsheet, and a dedicated `cron` topic gives the
    /// short answer for `floo docs cron`.
    #[test]
    fn test_cron_schema_documented_in_config_and_cron_topics() {
        for (label, content) in [("config/app-toml", CONFIG), ("cron", CRON)] {
            assert!(
                content.contains("[cron."),
                "{label} must show the [cron.<name>] section header",
            );
            for field in ["schedule", "command", "service", "timeout"] {
                assert!(
                    content.contains(field),
                    "{label} must document the '{field}' field",
                );
            }
            assert!(
                content.contains("floo cron list"),
                "{label} must show the read-only CLI surface",
            );
        }
        // The CONFIG cheatsheet must link to the long-form guide so agents
        // can route to it from `floo docs app-toml`.
        assert!(CONFIG.contains("getfloo.com/docs/guides/cron-jobs"));
    }

    #[test]
    fn test_cron_topic_in_overview_listing() {
        // The overview cheatsheet is the entry point; missing the cron topic
        // here was part of the original discoverability gap.
        assert!(render_overview().contains("floo docs cron"));
    }

    #[test]
    fn test_overview_has_key_concepts() {
        assert!(OVERVIEW.contains("Apps"));
        assert!(OVERVIEW.contains("Services"));
        assert!(OVERVIEW.contains("github connect"));
    }

    #[test]
    fn test_auth_docs_has_key_concepts() {
        // Gateway-managed accounts mode is the only documented public auth product.
        assert!(AUTH.contains("access_mode = \"accounts\""));
        assert!(AUTH.contains("X-Floo-User-Email"));
        assert!(AUTH.contains("/__floo/me"));
        assert!(AUTH.contains("/__floo/logout"));
        // OAuth-toolkit terminology must NOT appear in the public CLI docs.
        assert!(!AUTH.contains("redirect_uris"));
        assert!(!AUTH.contains("/v1/auth/apps"));
        assert!(!AUTH.contains("FLOO_APP_ID"));
        assert!(!AUTH.contains("access_token"));
    }

    #[test]
    fn test_quickstart_has_full_flow() {
        assert!(QUICKSTART.contains("auth login"));
        assert!(QUICKSTART.contains("init"));
        assert!(QUICKSTART.contains("github connect"));
        assert!(QUICKSTART.contains("apps show"));
        let preflight = QUICKSTART
            .find("floo preflight --json")
            .expect("quickstart must validate before the first deploy");
        let push = QUICKSTART
            .find("git push origin main")
            .expect("quickstart must push generated config");
        let connect = QUICKSTART
            .find("floo apps github connect owner/my-project")
            .expect("quickstart must connect GitHub");
        assert!(preflight < push && push < connect);
    }

    #[test]
    fn test_first_deploy_guidance_pushes_generated_config_before_connect() {
        for (name, content) in [
            ("overview", OVERVIEW),
            ("golden-path", HOWTO),
            ("templates", TEMPLATES),
            ("rails", RAILS),
            ("nextjs", NEXTJS),
            ("fastapi", FASTAPI),
            ("django", DJANGO),
            ("express", EXPRESS),
        ] {
            let push = content
                .find("git push origin main")
                .unwrap_or_else(|| panic!("{name} must push the generated config"));
            let connect = content
                .find("floo apps github connect")
                .unwrap_or_else(|| panic!("{name} must connect GitHub"));
            assert!(push < connect, "{name} must push before GitHub connect");
        }
    }

    #[test]
    fn test_services_no_coming_soon() {
        assert!(!SERVICES.contains("coming soon"));
    }

    #[test]
    fn test_scaling_topic_has_all_runtime_postures() {
        assert!(render_overview().contains("floo docs scaling"));
        for phrase in [
            "min_instances = 0",
            "min_instances = 1",
            "instances = 1",
            "instances = 0",
            "request-based CPU",
            "always-allocated CPU",
            "floo preflight --json",
            "floo preflight --env prod --json",
            "floo services show",
            "paid production",
            "$24.64/month",
        ] {
            assert!(
                SCALING.contains(phrase),
                "scaling topic is missing {phrase}"
            );
        }
    }

    #[test]
    fn test_deploy_mentions_github() {
        assert!(DEPLOY.contains("GitHub"));
        assert!(!DEPLOY.contains("archive"));
        assert!(!DEPLOY.contains("CLI sends metadata"));
        assert!(!DEPLOY.contains("full redeploy from local project"));
    }

    #[test]
    fn test_templates_has_react_fastapi() {
        assert!(TEMPLATES.contains("React + FastAPI"));
        assert!(TEMPLATES.contains("floo.app.toml"));
        assert!(TEMPLATES.contains("[services.web]"));
        assert!(TEMPLATES.contains("[services.api]"));
        assert!(TEMPLATES.contains("/api/users"));
        assert!(TEMPLATES.contains("VITE_API_URL"));
    }

    #[test]
    fn test_deploy_explains_dockerfiles() {
        assert!(DEPLOY.contains("Do I Need a Dockerfile?"));
        assert!(DEPLOY.contains("every service deploys from a Dockerfile"));
        assert!(DEPLOY.contains("floo init"));
    }

    #[test]
    fn test_services_explains_routing() {
        assert!(SERVICES.contains("gateway strips the /api"));
        assert!(SERVICES.contains("fetch(\"/api/users\")"));
        assert!(SERVICES.contains("unverified_localhost_fallback"));
        assert!(SERVICES.contains("hardcoded_localhost_fallback"));
    }

    #[test]
    fn test_doctor_documents_stable_offline_diagnostics() {
        assert!(DOCTOR.contains("floo doctor managed-services"));
        assert!(DOCTOR.contains("floo doctor accounts"));
        assert!(DOCTOR.contains("exits non-zero"));
        assert!(DOCTOR.contains("never includes Redis credentials"));
    }

    #[test]
    fn test_managed_service_guidance_separates_intent_from_destruction() {
        for (name, content) in [
            ("quickstart", QUICKSTART),
            ("services", SERVICES),
            ("config", CONFIG),
            ("templates", TEMPLATES),
        ] {
            assert!(
                content.contains("[managed.default]"),
                "{name} must use the default managed-service declaration when promising conventional env names"
            );
        }
        assert!(SERVICES.contains("does not delete provider data"));
        assert!(CONFIG.contains("Removing a deployed declaration"));
        assert!(SERVICES.contains("value is recorded and ignored"));
        assert!(QUICKSTART.contains("REDIS_URL_CACHE"));
        assert!(QUICKSTART.contains("STORAGE_BUCKET_UPLOADS"));
        assert!(!QUICKSTART.contains("authored via the CLI, not floo.app.toml"));
    }

    #[test]
    fn test_previews_topic_documents_agent_sandbox_contract() {
        assert!(render_overview().contains("floo docs previews"));
        assert!(PREVIEWS.contains("floo previews up"));
        assert!(PREVIEWS.contains("remote GitHub source only"));
        assert!(PREVIEWS.contains("dev_prod_untouched: true"));
        assert!(PREVIEWS.contains("managed_resource_branches"));
        assert!(PREVIEWS.contains("floo previews resources list"));
        assert!(PREVIEWS.contains("PREVIEW_MANAGED_SERVICE_ISOLATION_UNAVAILABLE"));
        assert!(PREVIEWS.contains("floo previews delete"));
        assert!(PREVIEWS.contains("getfloo.com/docs/cli/previews"));
    }

    #[test]
    fn test_egress_topic_documents_network_boundary_contract() {
        assert!(render_overview().contains("floo docs egress"));
        assert!(EGRESS.contains("floo's normal internet egress"));
        assert!(EGRESS.contains("Stable outbound source IP"));
        assert!(EGRESS.contains("SMTP port 25"));
        assert!(EGRESS.contains("customer-side connector"));
        assert!(EGRESS.contains("getfloo.com/docs/guides/networking"));
    }

    #[test]
    fn test_build_topic_lists_stack_guides() {
        assert!(BUILD.contains("floo docs nextjs"));
        assert!(BUILD.contains("floo docs rails"));
        assert!(BUILD.contains("floo docs fastapi"));
        assert!(BUILD.contains("floo docs django"));
        assert!(BUILD.contains("floo docs express"));
        assert!(BUILD.contains("getfloo.com/docs/build"));
    }

    #[test]
    fn test_rails_topic_covers_full_journey() {
        // Stack-journey shape: deploy → local dev → DB → auth → domain
        assert!(RAILS.contains("floo init"));
        assert!(RAILS.contains("[managed.default]"));
        assert!(RAILS.contains("access_mode = \"accounts\""));
        assert!(RAILS.contains("domains add"));
        assert!(RAILS.contains("floo dev"));
        // Rails workflow leans on rake/console/db:seed — floo run is the
        // way to do those with managed env. Mirror this surface to match
        // the published rails.mdx so agents reading via `floo docs rails`
        // see the same thing as agents reading via the docs site.
        assert!(RAILS.contains("floo run"));
        assert!(RAILS.contains("bin/rails console"));
    }

    #[test]
    fn test_overview_lists_build_journeys() {
        let overview = render_overview();
        assert!(overview.contains("floo docs build"));
        assert!(overview.contains("floo docs nextjs"));
        assert!(overview.contains("floo docs rails"));
        assert!(overview.contains("floo docs fastapi"));
        assert!(overview.contains("floo docs django"));
        assert!(overview.contains("floo docs express"));
    }

    #[test]
    fn test_all_stacks_cover_full_journey() {
        for (stack, content) in [
            ("nextjs", NEXTJS),
            ("rails", RAILS),
            ("fastapi", FASTAPI),
            ("django", DJANGO),
            ("express", EXPRESS),
        ] {
            assert!(
                content.contains("floo init"),
                "{stack}: missing 'floo init'"
            );
            assert!(
                content.contains("[managed.default]"),
                "{stack}: missing the managed Postgres declaration"
            );
            assert!(
                content.contains("access_mode = \"accounts\""),
                "{stack}: missing access_mode"
            );
            assert!(
                content.contains("domains add"),
                "{stack}: missing 'domains add'"
            );
            assert!(content.contains("floo dev"), "{stack}: missing 'floo dev'");
            assert!(
                content.contains("getfloo.com/docs/build"),
                "{stack}: missing link to full guide"
            );
        }
    }

    #[test]
    fn test_all_stacks_use_gateway_managed_auth_only() {
        // Every stack guide shows gateway-managed accounts mode and NOTHING else
        // — the OAuth toolkit is not a documented public product.
        for (stack, content) in [
            ("nextjs", NEXTJS),
            ("rails", RAILS),
            ("fastapi", FASTAPI),
            ("django", DJANGO),
            ("express", EXPRESS),
        ] {
            // Gateway-managed: app reads injected identity header.
            assert!(
                content.to_lowercase().contains("x-floo-user-email")
                    || content.contains("X-Floo-User-Email"),
                "{stack}: missing X-Floo-User-Email"
            );
            // OAuth-toolkit terminology must NOT appear.
            assert!(
                !content.contains("redirect_uris"),
                "{stack}: leaked redirect_uris"
            );
            assert!(
                !content.contains("/v1/auth/apps"),
                "{stack}: leaked OAuth endpoint"
            );
            assert!(
                !content.contains("FLOO_APP_ID"),
                "{stack}: leaked FLOO_APP_ID"
            );
            assert!(
                !content.contains("hosted app OAuth"),
                "{stack}: leaked 'hosted app OAuth'"
            );
            assert!(
                !content.contains("Hosted app OAuth"),
                "{stack}: leaked 'Hosted app OAuth'"
            );
        }
    }

    /// Every `floo docs <topic>` mention across the overview and every topic
    /// body must resolve to a real topic or alias. The dead
    /// `floo docs state-model` cross-reference (#1159) is exactly what this
    /// pins — the whole class, not just that one instance.
    #[test]
    fn test_every_floo_docs_cross_reference_resolves() {
        let valid: std::collections::HashSet<&str> = TOPICS
            .iter()
            .flat_map(|topic| std::iter::once(topic.name).chain(topic.aliases.iter().copied()))
            .collect();
        let re = regex::Regex::new(r"floo docs ([a-z][a-z-]*)").unwrap();
        let mut bodies: Vec<&str> = TOPICS.iter().map(|topic| topic.content).collect();
        let overview = render_overview();
        bodies.push(&overview);
        for body in bodies {
            for cap in re.captures_iter(body) {
                let referenced = cap.get(1).unwrap().as_str();
                assert!(
                    valid.contains(referenced),
                    "cross-reference `floo docs {referenced}` has no matching topic or alias",
                );
            }
        }
    }

    /// Every canonical topic must be listed in the overview so an agent reading
    /// `floo docs` can discover all of them. `notifications` was invisible here
    /// before #1159.
    #[test]
    fn test_overview_lists_every_canonical_topic() {
        let overview = render_overview();
        for topic in TOPICS {
            assert!(
                overview.contains(&format!("floo docs {}", topic.name)),
                "overview is missing `floo docs {}`",
                topic.name,
            );
        }
    }

    /// Every alias must resolve to a real canonical topic, must not shadow a
    /// canonical name, and must be surfaced in the overview so it stays
    /// discoverable rather than being a hidden duplicate (#1159).
    #[test]
    fn test_every_alias_resolves_to_canonical_topic() {
        let overview = render_overview();
        for topic in TOPICS {
            for alias in topic.aliases {
                assert!(
                    !TOPICS.iter().any(|candidate| candidate.name == *alias),
                    "alias `{alias}` collides with a canonical topic name",
                );
                assert!(
                    overview.contains(alias),
                    "alias `{alias}` for `{}` is not surfaced in the overview",
                    topic.name,
                );
            }
        }
    }

    /// Dispatching an alias returns the canonical topic's name and content; an
    /// unknown topic returns None so the caller shows the available-topics hint.
    #[test]
    fn test_alias_dispatch_resolves_to_canonical() {
        let (services, storage_is_alias) = resolve_topic("storage").unwrap();
        assert_eq!(services.name, "services");
        assert_eq!(services.content, SERVICES);
        assert!(storage_is_alias);

        let (config, app_toml_is_alias) = resolve_topic("app-toml").unwrap();
        assert_eq!(config.name, "config");
        assert_eq!(config.content, CONFIG);
        assert!(app_toml_is_alias);

        let (services, services_is_alias) = resolve_topic("services").unwrap();
        assert_eq!(services.name, "services");
        assert!(!services_is_alias);
        assert!(resolve_topic("state-model").is_none());
        assert!(resolve_topic("definitely-not-a-topic").is_none());
    }

    #[test]
    fn test_topic_registry_is_well_formed() {
        let mut identifiers = std::collections::HashSet::new();
        for topic in TOPICS {
            assert!(!topic.name.is_empty(), "topic name must not be empty");
            assert!(
                identifiers.insert(topic.name),
                "duplicate topic or alias `{}`",
                topic.name,
            );
            assert!(
                !topic.summary.trim().is_empty(),
                "topic `{}` has no summary",
                topic.name,
            );
            assert!(
                !topic.content.trim().is_empty(),
                "topic `{}` has no content",
                topic.name,
            );
            for alias in topic.aliases {
                assert!(
                    identifiers.insert(alias),
                    "duplicate topic or alias `{alias}`",
                );
            }
        }
    }

    #[test]
    fn test_embedded_docs_stay_within_size_budget() {
        let total_bytes = OVERVIEW.len()
            + TOPICS
                .iter()
                .map(|topic| topic.content.len())
                .sum::<usize>();
        assert!(
            total_bytes <= MAX_EMBEDDED_DOCS_BYTES,
            "embedded docs are {total_bytes} bytes, above the {MAX_EMBEDDED_DOCS_BYTES}-byte budget",
        );
    }

    #[test]
    fn test_topic_index_json_covers_registry() {
        let index = topic_index_json();
        assert_eq!(index.len(), TOPICS.len());
        for (entry, topic) in index.iter().zip(TOPICS) {
            assert_eq!(entry["name"], topic.name);
            assert_eq!(entry["summary"], topic.summary);
            assert_eq!(entry["aliases"], serde_json::json!(topic.aliases));
        }
    }
}
