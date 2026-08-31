use crate::{constants::VERSION, output};

const DOCS_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Topic {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) aliases: &'static [&'static str],
    /// Path of the canonical page on the docs site, relative to `DOCS_BASE`.
    ///
    /// `floo docs` routes; it does not carry prose. Guidance lived in two
    /// places — Markdown embedded in this binary and the site — and the copies
    /// drifted: the embedded text still told agents to use a minted setup link
    /// as an install link long after that was known to be wrong. One source of
    /// truth is worth more than offline access.
    pub(crate) path: &'static str,
}

impl Topic {
    pub(crate) fn url(&self) -> String {
        format!("{DOCS_BASE}/{}", self.path)
    }
}

const DOCS_BASE: &str = "https://getfloo.com/docs";

const NO_ALIASES: &[&str] = &[];
const SERVICES_ALIASES: &[&str] = &["storage"];
const CONFIG_ALIASES: &[&str] = &["app-toml"];

/// Canonical topics in display order. This registry owns discovery, aliases,
/// summaries, and the documentation page each topic resolves to.
pub(crate) const TOPICS: &[Topic] = &[
    Topic {
        name: "golden-path",
        summary: "Golden path and decision table",
        aliases: NO_ALIASES,
        path: "guides/cli",
    },
    Topic {
        name: "quickstart",
        summary: "End-to-end setup and first deploy",
        aliases: NO_ALIASES,
        path: "introduction",
    },
    Topic {
        name: "build",
        summary: "Runtime images, Dockerfiles, and build caching",
        aliases: NO_ALIASES,
        path: "guides/dockerfiles",
    },
    Topic {
        name: "nextjs",
        summary: "Build and deploy a Next.js app",
        aliases: NO_ALIASES,
        path: "build/nextjs",
    },
    Topic {
        name: "rails",
        summary: "Build and deploy a Rails app",
        aliases: NO_ALIASES,
        path: "build/rails",
    },
    Topic {
        name: "fastapi",
        summary: "Build and deploy a FastAPI app",
        aliases: NO_ALIASES,
        path: "build/fastapi",
    },
    Topic {
        name: "django",
        summary: "Build and deploy a Django app",
        aliases: NO_ALIASES,
        path: "build/django",
    },
    Topic {
        name: "express",
        summary: "Build and deploy an Express app",
        aliases: NO_ALIASES,
        path: "build/express",
    },
    Topic {
        name: "templates",
        summary: "Copy-paste app structures",
        aliases: NO_ALIASES,
        path: "guides/configuration",
    },
    Topic {
        name: "services",
        summary: "App services and managed services",
        aliases: SERVICES_ALIASES,
        path: "guides/managed-services",
    },
    Topic {
        name: "doctor",
        summary: "Managed-service health and accounts drift",
        aliases: NO_ALIASES,
        path: "cli/doctor",
    },
    Topic {
        name: "edge",
        summary: "Routes, access rules, and edge enforcement",
        aliases: NO_ALIASES,
        path: "cli/edge",
    },
    Topic {
        name: "egress",
        summary: "Outbound networking and private-network status",
        aliases: NO_ALIASES,
        path: "guides/networking",
    },
    Topic {
        name: "previews",
        summary: "Remote-branch preview sandboxes",
        aliases: NO_ALIASES,
        path: "guides/preview-environments",
    },
    Topic {
        name: "config",
        summary: "Configuration file formats and examples",
        aliases: CONFIG_ALIASES,
        path: "reference/config-spec",
    },
    Topic {
        name: "scaling",
        summary: "Availability, scaling, and CPU behavior",
        aliases: NO_ALIASES,
        path: "guides/scaling",
    },
    Topic {
        name: "cron",
        summary: "Scheduled-job schema and operations",
        aliases: NO_ALIASES,
        path: "guides/cron-jobs",
    },
    Topic {
        name: "deploy",
        summary: "Git-driven deploy lifecycle and detection",
        aliases: NO_ALIASES,
        path: "how-floo-works",
    },
    Topic {
        name: "auth",
        summary: "Authentication for floo and hosted apps",
        aliases: NO_ALIASES,
        path: "cli/auth",
    },
    Topic {
        name: "notifications",
        summary: "Deployment email preferences",
        aliases: NO_ALIASES,
        path: "cli/notifications",
    },
    Topic {
        name: "github",
        summary: "Grant the GitHub App access to a repo, and bind it to your org",
        aliases: NO_ALIASES,
        path: "cli/github",
    },
    Topic {
        name: "feedback",
        summary: "Report product friction from the CLI",
        aliases: NO_ALIASES,
        path: "cli/feedback",
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
                "Available topics in floo {VERSION}: {}. Full documentation: {DOCS_BASE}",
                available.join(", ")
            )),
        );
        std::process::exit(1);
    };

    if !output::is_json_mode() {
        eprintln!("{}  {}", topic.summary, topic.url());
        return;
    }

    let mut data = serde_json::json!({
        "schema_version": DOCS_SCHEMA_VERSION,
        "cli_version": VERSION,
        "topic": topic.name,
        "summary": topic.summary,
        "url": topic.url(),
    });
    if is_alias {
        data["requested_topic"] = serde_json::json!(requested);
        data["alias"] = serde_json::json!(true);
    }
    output::success(&format!("docs:{}", topic.name), Some(data));
}

fn render_overview() -> String {
    let mut rendered = format!(
        "floo documentation is published at {DOCS_BASE}.\n\n\
         `floo docs <topic>` prints the canonical page for a topic. The CLI \
         routes to the docs rather than embedding a copy, so guidance cannot \
         drift between the binary you have and the site.\n\n## Topics\n\n"
    );
    for topic in TOPICS {
        let aliases = if topic.aliases.is_empty() {
            String::new()
        } else {
            format!(" (alias: {})", topic.aliases.join(", "))
        };
        rendered.push_str(&format!(
            "  floo docs {:<14} {}{aliases}\n                 {}\n",
            topic.name,
            topic.summary,
            topic.url()
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
                "url": topic.url(),
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
    use super::{render_overview, resolve_topic, topic_index_json, DOCS_BASE, TOPICS};

    /// The prose assertions that used to live here moved with the prose. The
    /// docs repo validates page content; this module now validates routing —
    /// that every topic resolves to a real, well-formed page URL.
    #[test]
    fn test_topic_registry_is_well_formed() {
        assert!(!TOPICS.is_empty());
        for topic in TOPICS {
            assert!(!topic.name.is_empty(), "topic name must not be empty");
            assert!(!topic.summary.is_empty(), "{} has no summary", topic.name);
            assert!(!topic.path.is_empty(), "{} has no path", topic.name);
            assert!(
                !topic.path.starts_with('/'),
                "{} path must be relative to DOCS_BASE, got {}",
                topic.name,
                topic.path
            );
            assert!(
                !topic.path.ends_with(".md") && !topic.path.ends_with(".mdx"),
                "{} path must be a page route, not a file: {}",
                topic.name,
                topic.path
            );
        }
    }

    #[test]
    fn test_topic_names_and_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for topic in TOPICS {
            assert!(seen.insert(topic.name), "duplicate topic {}", topic.name);
            for alias in topic.aliases {
                assert!(seen.insert(alias), "alias {alias} collides");
            }
        }
    }

    #[test]
    fn test_url_is_composed_from_the_docs_base() {
        let topic = TOPICS
            .iter()
            .find(|t| t.name == "github")
            .expect("github topic");
        assert_eq!(topic.url(), format!("{DOCS_BASE}/cli/github"));
        assert!(topic.url().starts_with("https://"));
    }

    #[test]
    fn test_github_topic_routes_to_the_grant_guidance() {
        // The failure that motivated the router: an agent had no way to reach
        // the page explaining that the App must be installed on the account
        // that owns the repo.
        let topic = TOPICS.iter().find(|t| t.name == "github");
        assert!(topic.is_some(), "github must be a discoverable topic");
        assert_eq!(topic.unwrap().path, "cli/github");
    }

    #[test]
    fn test_every_alias_resolves_to_canonical_topic() {
        for topic in TOPICS {
            for alias in topic.aliases {
                let (resolved, is_alias) = resolve_topic(alias).expect("alias must resolve");
                assert_eq!(resolved.name, topic.name);
                assert!(is_alias, "{alias} must report as an alias");
            }
            let (resolved, is_alias) =
                resolve_topic(topic.name).expect("canonical name must resolve");
            assert_eq!(resolved.name, topic.name);
            assert!(!is_alias, "{} must not report as an alias", topic.name);
        }
    }

    #[test]
    fn test_unknown_topic_does_not_resolve() {
        assert!(resolve_topic("not-a-topic").is_none());
    }

    #[test]
    fn test_overview_lists_every_topic_with_its_url() {
        let overview = render_overview();
        for topic in TOPICS {
            assert!(
                overview.contains(topic.name),
                "overview omits topic {}",
                topic.name
            );
            assert!(
                overview.contains(&topic.url()),
                "overview omits the URL for {}",
                topic.name
            );
        }
        assert!(overview.contains(DOCS_BASE));
    }

    #[test]
    fn test_topic_index_json_covers_registry_with_urls() {
        let index = topic_index_json();
        assert_eq!(index.len(), TOPICS.len());
        for (entry, topic) in index.iter().zip(TOPICS) {
            assert_eq!(entry["name"], topic.name);
            assert_eq!(entry["summary"], topic.summary);
            assert_eq!(entry["url"], topic.url());
        }
    }

    /// Guard the whole point of this change: prose must not come back into the
    /// binary. Two copies of guidance is how the CLI ended up telling agents to
    /// use a minted setup link as an install link long after that was wrong.
    #[test]
    fn test_no_offline_prose_is_embedded() {
        let source = include_str!("docs.rs");
        assert!(
            !source.contains("include_str!(\"../../docs/offline"),
            "docs topics must route to the site, not embed a second copy"
        );
    }
}
