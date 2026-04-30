//! Container runtime — `floo dev` and `floo run` execute every command inside
//! a Docker (or Podman) container built from the service's Dockerfile, so
//! the local toolchain matches what ships to production.
//!
//! Two design rules govern this module:
//!
//! 1. **The Dockerfile is the contract.** No silent fallback to host shell:
//!    if a service has no Dockerfile, the command refuses to run and tells
//!    the caller how to add one. Mixing host-shell and in-container runs
//!    creates a "works on my machine" bug class we explicitly opt out of.
//!
//! 2. **The image tag is content-addressed.** A SHA-256 of the Dockerfile
//!    plus every common lockfile in the build context determines the tag.
//!    Same content → same tag → no rebuild. Editing `package-lock.json`
//!    invalidates the tag, forcing a rebuild that picks up new deps.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

use sha2::{Digest, Sha256};

use crate::errors::{ErrorCode, FlooError};

/// Container runtime — Docker or its drop-in replacement Podman.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Docker,
    Podman,
}

impl Runtime {
    pub fn binary(self) -> &'static str {
        match self {
            Runtime::Docker => "docker",
            Runtime::Podman => "podman",
        }
    }
}

impl std::fmt::Display for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.binary())
    }
}

/// Default in-container working directory when the Dockerfile doesn't
/// declare one. Matches the convention `dockerfile.rs` uses when generating
/// Dockerfiles for `floo init`.
pub const DEFAULT_WORKDIR: &str = "/app";

/// Lockfiles whose content participates in the image-tag hash so dependency
/// edits force a rebuild. Listed in alphabetical order; each is hashed only
/// if present in the service path.
const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "Gemfile.lock",
    "go.sum",
    "package-lock.json",
    "pnpm-lock.yaml",
    "poetry.lock",
    "pyproject.toml",
    "requirements.txt",
    "uv.lock",
    "yarn.lock",
];

/// Container-internal paths that should be preserved from the image (i.e. not
/// shadowed by the source bind mount). Without this, mounting host source
/// over `/app` would hide `/app/node_modules` baked in at build time.
///
/// We hardcode the common ecosystems rather than detect — the cost of an
/// unused anonymous volume is zero, and every dev image benefits from
/// covering the languages we support.
const PRESERVED_DEPS_PATHS: &[&str] = &[
    ".bundle",
    ".cache",
    ".next",
    ".venv",
    "__pycache__",
    "node_modules",
    "target",
    "vendor/bundle",
];

/// Probe for an available container runtime. Prefers Docker; falls back to
/// Podman. The presence check is two-stage — `which` first, then `info` to
/// confirm the daemon is reachable. A binary on PATH that can't talk to its
/// daemon is just as useless as no binary at all.
pub fn detect_runtime() -> Result<Runtime, FlooError> {
    for runtime in [Runtime::Docker, Runtime::Podman] {
        if which(runtime.binary()).is_none() {
            continue;
        }
        let info_ok = Command::new(runtime.binary())
            .arg("info")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if info_ok {
            return Ok(runtime);
        }
    }
    Err(FlooError::with_suggestion(
        ErrorCode::ContainerRuntimeUnavailable,
        "No container runtime available — floo dev/run requires Docker or Podman.".to_string(),
        "Install Docker (https://docs.docker.com/get-docker/) or Podman (https://podman.io) and \
         make sure the daemon is running."
            .to_string(),
    ))
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Compute the content-addressed hash that drives image tagging. Inputs are
/// the Dockerfile content plus every lockfile that exists in the service path.
pub fn compute_build_hash(dockerfile_content: &str, service_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"floo-dev-image-v1\0");
    hasher.update(b"DOCKERFILE\0");
    hasher.update(dockerfile_content.as_bytes());
    for name in LOCKFILES {
        let path = service_dir.join(name);
        if let Ok(content) = fs::read(&path) {
            hasher.update(b"\0LOCK\0");
            hasher.update(name.as_bytes());
            hasher.update(b"\0");
            hasher.update(&content);
        }
    }
    let bytes = hasher.finalize();
    bytes.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Image tag floo uses for a service's dev image. Stable across runs as long
/// as the inputs to `compute_build_hash` haven't changed.
pub fn image_tag(app: &str, service: &str, build_hash: &str) -> String {
    let safe_app = sanitize_for_tag(app);
    let safe_service = sanitize_for_tag(service);
    format!("floo-dev-{safe_app}-{safe_service}:{build_hash}")
}

fn sanitize_for_tag(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "x".to_string()
    } else {
        cleaned
    }
}

/// Best-effort parse of the last `WORKDIR` directive from a Dockerfile.
/// Returns `None` if no statically-resolvable WORKDIR is set.
pub fn parse_workdir(content: &str) -> Option<String> {
    let mut last = None;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let directive = parts.next()?;
        if !directive.eq_ignore_ascii_case("WORKDIR") {
            continue;
        }
        let Some(rest) = parts.next() else { continue };
        let path = rest.trim().trim_matches('"').trim_matches('\'');
        // A `WORKDIR ${BUILD_DIR}` reference can't be resolved without
        // running the build — skip it and keep the previous value.
        if path.contains('$') {
            continue;
        }
        last = Some(path.to_string());
    }
    last
}

/// What a single `docker run` invocation looks like.
pub struct RunSpec {
    /// Image to run, typically the result of `image_tag()`.
    pub image: String,
    /// Path inside the container that the source bind mount targets. Should
    /// match the image's WORKDIR.
    pub workdir_in_container: String,
    /// Absolute host directory that bind-mounts onto `workdir_in_container`.
    pub source_mount_host: PathBuf,
    /// Env vars passed via `-e KEY=VAL`. Sorted-key for stable argv.
    pub env: BTreeMap<String, String>,
    /// Shell command run inside the container as `sh -c <command>`.
    pub command: String,
    /// Host port → container port mappings, published only on `127.0.0.1`.
    pub ports: BTreeMap<u16, u16>,
    /// Container-internal paths that must survive the source bind mount.
    /// Created as anonymous volumes so they inherit from the image.
    pub preserved_paths: Vec<String>,
    /// Container name. Required so we can `docker stop <name>` on shutdown.
    pub name: String,
    /// Allocate stdin to the container.
    pub interactive: bool,
    /// Allocate a TTY.
    pub tty: bool,
    /// Pass `--init` so the runtime forwards SIGTERM to PID 1's children.
    pub init: bool,
}

impl RunSpec {
    /// Translate to the argv that follows the runtime binary in `Command::new`.
    /// Pure transform — kept separate so unit tests can assert exact argv.
    pub fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            self.name.clone(),
        ];
        if self.init {
            args.push("--init".to_string());
        }
        if self.interactive {
            args.push("-i".to_string());
        }
        if self.tty {
            args.push("-t".to_string());
        }

        args.push("-w".to_string());
        args.push(self.workdir_in_container.clone());

        let host = self.source_mount_host.display().to_string();
        args.push("-v".to_string());
        args.push(format!("{host}:{}", self.workdir_in_container));

        for path in &self.preserved_paths {
            let abs = if path.starts_with('/') {
                path.clone()
            } else {
                format!(
                    "{}/{}",
                    self.workdir_in_container.trim_end_matches('/'),
                    path
                )
            };
            args.push("-v".to_string());
            args.push(abs);
        }

        for (k, v) in &self.env {
            args.push("-e".to_string());
            args.push(format!("{k}={v}"));
        }

        for (host_port, container_port) in &self.ports {
            args.push("-p".to_string());
            args.push(format!("127.0.0.1:{host_port}:{container_port}"));
        }

        args.push(self.image.clone());
        args.push("sh".to_string());
        args.push("-c".to_string());
        args.push(self.command.clone());
        args
    }
}

/// Whether an image with this tag is already present locally. Used to skip
/// unnecessary `docker build` on cache hits.
pub fn image_exists(runtime: Runtime, tag: &str) -> bool {
    Command::new(runtime.binary())
        .args(["image", "inspect", tag])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub struct BuildSpec {
    pub tag: String,
    pub context_dir: PathBuf,
    pub dockerfile: PathBuf,
}

/// Build the image. Stdout/stderr inherited so build progress is visible to
/// the user. Returns once the build finishes.
pub fn build_image(runtime: Runtime, spec: &BuildSpec) -> Result<(), FlooError> {
    let status = Command::new(runtime.binary())
        .arg("build")
        .arg("-t")
        .arg(&spec.tag)
        .arg("-f")
        .arg(&spec.dockerfile)
        .arg(&spec.context_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| {
            FlooError::with_suggestion(
                ErrorCode::ContainerBuildFailed,
                format!("Failed to invoke {}: {e}", runtime.binary()),
                format!("Is {} installed and on PATH?", runtime.binary()),
            )
        })?;

    if !status.success() {
        return Err(FlooError::with_suggestion(
            ErrorCode::ContainerBuildFailed,
            format!(
                "{} build failed with exit code {}.",
                runtime.binary(),
                status.code().unwrap_or(-1)
            ),
            format!(
                "Reproduce the error directly: {} build -f {} {}",
                runtime.binary(),
                spec.dockerfile.display(),
                spec.context_dir.display()
            ),
        ));
    }
    Ok(())
}

/// Run the container in the foreground, inheriting stdio. Blocks until the
/// container exits. Used by `floo run` for one-shot commands.
pub fn run_foreground(runtime: Runtime, spec: &RunSpec) -> Result<ExitStatus, FlooError> {
    Command::new(runtime.binary())
        .args(spec.to_args())
        .stdin(if spec.interactive {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| {
            FlooError::new(
                ErrorCode::InternalError,
                format!("Failed to invoke {}: {e}", runtime.binary()),
            )
        })
}

/// Spawn the container with stdout/stderr piped for stream multiplexing.
/// Used by `floo dev`, which prefixes output per service.
pub fn spawn_piped(runtime: Runtime, spec: &RunSpec) -> Result<Child, FlooError> {
    Command::new(runtime.binary())
        .args(spec.to_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            FlooError::new(
                ErrorCode::InternalError,
                format!("Failed to invoke {}: {e}", runtime.binary()),
            )
        })
}

/// Best-effort `docker stop <name>` so the container exits gracefully when
/// the user hits Ctrl-C. Paired with `--rm`, the container is also removed.
pub fn stop_container(runtime: Runtime, name: &str) {
    let _ = Command::new(runtime.binary())
        .args(["stop", "--time", "10", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Generate a unique container name. The hash suffix lets parallel `floo dev`
/// invocations on the same app coexist without name collisions.
pub fn container_name(app: &str, service: &str) -> String {
    use rand::RngCore;
    let mut buf = [0u8; 3];
    rand::thread_rng().fill_bytes(&mut buf);
    let suffix: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "floo-dev-{}-{}-{suffix}",
        sanitize_for_tag(app),
        sanitize_for_tag(service)
    )
}

/// The set of paths that should be preserved as anonymous volumes when
/// bind-mounting host source over the container's WORKDIR.
pub fn default_preserved_paths() -> Vec<String> {
    PRESERVED_DEPS_PATHS.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn dockerfile_hash_changes_with_content() {
        let dir = TempDir::new().unwrap();
        let h1 = compute_build_hash("FROM node:20", dir.path());
        let h2 = compute_build_hash("FROM node:21", dir.path());
        assert_ne!(h1, h2);
    }

    #[test]
    fn dockerfile_hash_stable_for_same_inputs() {
        let dir = TempDir::new().unwrap();
        let h1 = compute_build_hash("FROM node:20", dir.path());
        let h2 = compute_build_hash("FROM node:20", dir.path());
        assert_eq!(h1, h2);
    }

    #[test]
    fn dockerfile_hash_changes_when_lockfile_changes() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package-lock.json"), "{\"v\": 1}").unwrap();
        let h1 = compute_build_hash("FROM node:20", dir.path());
        fs::write(dir.path().join("package-lock.json"), "{\"v\": 2}").unwrap();
        let h2 = compute_build_hash("FROM node:20", dir.path());
        assert_ne!(h1, h2);
    }

    #[test]
    fn dockerfile_hash_ignores_unrelated_files() {
        let dir = TempDir::new().unwrap();
        let h1 = compute_build_hash("FROM node:20", dir.path());
        fs::write(dir.path().join("README.md"), "hello").unwrap();
        let h2 = compute_build_hash("FROM node:20", dir.path());
        assert_eq!(h1, h2);
    }

    #[test]
    fn image_tag_is_lowercased_and_safe() {
        let tag = image_tag("My App", "Web/Service", "deadbe");
        assert_eq!(tag, "floo-dev-my-app-web-service:deadbe");
    }

    #[test]
    fn image_tag_handles_empty_component() {
        let tag = image_tag("", "web", "abc");
        assert_eq!(tag, "floo-dev-x-web:abc");
    }

    #[test]
    fn parse_workdir_returns_last_directive() {
        let dockerfile = "
FROM node:20
WORKDIR /tmp
RUN echo hi
WORKDIR /srv/app
";
        assert_eq!(parse_workdir(dockerfile).as_deref(), Some("/srv/app"));
    }

    #[test]
    fn parse_workdir_strips_quotes() {
        assert_eq!(
            parse_workdir("FROM scratch\nWORKDIR \"/quoted\"\n").as_deref(),
            Some("/quoted")
        );
    }

    #[test]
    fn parse_workdir_skips_unresolved_args() {
        let dockerfile = "
FROM node:20
WORKDIR /first
WORKDIR ${BUILD_DIR}
";
        assert_eq!(parse_workdir(dockerfile).as_deref(), Some("/first"));
    }

    #[test]
    fn parse_workdir_returns_none_when_absent() {
        assert!(parse_workdir("FROM scratch\nRUN echo hi\n").is_none());
    }

    #[test]
    fn parse_workdir_ignores_comments() {
        let dockerfile = "
# WORKDIR /commented-out
FROM node:20
WORKDIR /real
";
        assert_eq!(parse_workdir(dockerfile).as_deref(), Some("/real"));
    }

    #[test]
    fn run_spec_argv_includes_required_flags() {
        let mut env = BTreeMap::new();
        env.insert("DATABASE_URL".to_string(), "postgres://x".to_string());
        let mut ports = BTreeMap::new();
        ports.insert(3000u16, 3000u16);
        let spec = RunSpec {
            image: "floo-dev-app-web:abc".to_string(),
            workdir_in_container: "/app".to_string(),
            source_mount_host: PathBuf::from("/repo/web"),
            env,
            command: "npm run dev".to_string(),
            ports,
            preserved_paths: vec!["node_modules".to_string()],
            name: "floo-dev-app-web-aaaaaa".to_string(),
            interactive: false,
            tty: false,
            init: true,
        };
        let args = spec.to_args();
        // `run --rm --name <name>` always comes first.
        assert_eq!(
            &args[0..4],
            &["run", "--rm", "--name", "floo-dev-app-web-aaaaaa"]
        );
        assert!(args.contains(&"--init".to_string()));
        // Bind mount and anonymous volume both present.
        assert!(args.iter().any(|a| a == "/repo/web:/app"));
        assert!(args.iter().any(|a| a == "/app/node_modules"));
        // Env passed through.
        assert!(args.iter().any(|a| a == "DATABASE_URL=postgres://x"));
        // Port published on loopback only.
        assert!(args.iter().any(|a| a == "127.0.0.1:3000:3000"));
        // Command is `sh -c <cmd>` at the very end.
        let n = args.len();
        assert_eq!(&args[n - 3..], &["sh", "-c", "npm run dev"]);
    }

    #[test]
    fn run_spec_argv_omits_init_and_tty_when_disabled() {
        let spec = RunSpec {
            image: "img:tag".to_string(),
            workdir_in_container: "/app".to_string(),
            source_mount_host: PathBuf::from("/host"),
            env: BTreeMap::new(),
            command: "true".to_string(),
            ports: BTreeMap::new(),
            preserved_paths: vec![],
            name: "n".to_string(),
            interactive: false,
            tty: false,
            init: false,
        };
        let args = spec.to_args();
        assert!(!args.contains(&"--init".to_string()));
        assert!(!args.contains(&"-t".to_string()));
        assert!(!args.contains(&"-i".to_string()));
    }

    #[test]
    fn run_spec_preserved_paths_resolve_relative_to_workdir() {
        let spec = RunSpec {
            image: "img:tag".to_string(),
            workdir_in_container: "/srv/app/".to_string(),
            source_mount_host: PathBuf::from("/host"),
            env: BTreeMap::new(),
            command: "true".to_string(),
            ports: BTreeMap::new(),
            preserved_paths: vec!["node_modules".to_string(), "/abs/path".to_string()],
            name: "n".to_string(),
            interactive: false,
            tty: false,
            init: false,
        };
        let args = spec.to_args();
        assert!(args.iter().any(|a| a == "/srv/app/node_modules"));
        assert!(args.iter().any(|a| a == "/abs/path"));
    }

    #[test]
    fn container_name_includes_app_and_service() {
        let name = container_name("my-app", "web");
        assert!(name.starts_with("floo-dev-my-app-web-"));
        // Six hex chars of suffix.
        let suffix = name.trim_start_matches("floo-dev-my-app-web-");
        assert_eq!(suffix.len(), 6);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn default_preserved_paths_includes_common_ecosystems() {
        let paths = default_preserved_paths();
        assert!(paths.iter().any(|p| p == "node_modules"));
        assert!(paths.iter().any(|p| p == ".venv"));
        assert!(paths.iter().any(|p| p == "vendor/bundle"));
        assert!(paths.iter().any(|p| p == "target"));
    }
}
