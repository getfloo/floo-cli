use std::path::Path;

use serde_json::Value;

use crate::detection::DetectionResult;

#[derive(Debug, Clone, Copy, PartialEq)]
enum PackageManager {
    Npm,
    Yarn,
    Pnpm,
}

impl PackageManager {
    fn copy_line(&self) -> &str {
        match self {
            PackageManager::Npm => "COPY package.json package-lock.json* ./",
            PackageManager::Yarn => "COPY package.json yarn.lock* ./",
            PackageManager::Pnpm => "COPY package.json pnpm-lock.yaml* ./",
        }
    }

    fn install_cmd(&self) -> &str {
        match self {
            PackageManager::Npm => "RUN npm ci",
            PackageManager::Yarn => "RUN yarn install --frozen-lockfile",
            PackageManager::Pnpm => "RUN pnpm install --frozen-lockfile",
        }
    }

    fn run_build(&self) -> &str {
        match self {
            PackageManager::Npm => "RUN npm run build",
            PackageManager::Yarn => "RUN yarn build",
            PackageManager::Pnpm => "RUN pnpm build",
        }
    }

    fn start_cmd(&self) -> Vec<&str> {
        match self {
            PackageManager::Npm => vec!["npm", "start"],
            PackageManager::Yarn => vec!["yarn", "start"],
            PackageManager::Pnpm => vec!["pnpm", "start"],
        }
    }
}

fn detect_package_manager(path: &Path) -> PackageManager {
    if path.join("pnpm-lock.yaml").exists() {
        PackageManager::Pnpm
    } else if path.join("yarn.lock").exists() {
        PackageManager::Yarn
    } else {
        PackageManager::Npm
    }
}

fn detect_node_entry_point(path: &Path) -> String {
    // Check package.json main field
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        if let Ok(pkg) = serde_json::from_str::<Value>(&content) {
            if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
                if !main.is_empty() {
                    return main.to_string();
                }
            }
        }
    }

    // Scan for common entry points
    let candidates = ["src/index.js", "index.js", "server.js", "app.js"];
    for candidate in &candidates {
        if path.join(candidate).exists() {
            return candidate.to_string();
        }
    }

    "src/index.js".to_string()
}

fn detect_python_entry_point(path: &Path) -> String {
    // Check pyproject.toml for module hints
    if let Ok(content) = std::fs::read_to_string(path.join("pyproject.toml")) {
        // Look for a [tool.uvicorn] or common patterns
        // Check for module reference like "app.main:app" in scripts
        for line in content.lines() {
            let trimmed = line.trim();
            // Match patterns like: module = "myapp.main:app"
            if trimmed.contains(":app") || trimmed.contains(":application") {
                if let Some(val) = trimmed.split('=').nth(1) {
                    let cleaned = val.trim().trim_matches('"').trim_matches('\'');
                    if cleaned.contains(':') {
                        return cleaned.to_string();
                    }
                }
            }
        }
    }

    // Scan for common entry points
    let candidates = [
        ("app/main.py", "app.main:app"),
        ("main.py", "main:app"),
        ("app.py", "app:app"),
    ];
    for (file, module) in &candidates {
        if path.join(file).exists() {
            return module.to_string();
        }
    }

    "app.main:app".to_string()
}

fn detect_python_version(path: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path.join("pyproject.toml")) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("requires-python") {
                // Extract version like ">= 3.11" or ">=3.12"
                if let Some(val) = trimmed.split('=').next_back() {
                    let version = val
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .trim_start_matches(">=")
                        .trim_start_matches(">")
                        .trim_start_matches("~=")
                        .trim();
                    // Take major.minor only
                    let parts: Vec<&str> = version.split('.').collect();
                    if parts.len() >= 2 {
                        if let (Ok(_major), Ok(_minor)) =
                            (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                        {
                            return format!("{}.{}", parts[0], parts[1]);
                        }
                    }
                }
            }
        }
    }

    "3.13".to_string()
}

/// Generate a Dockerfile for the detected runtime/framework.
/// Returns None if a Dockerfile already exists (runtime == "docker") or runtime is unknown.
pub fn generate_dockerfile(detection: &DetectionResult, project_path: &Path) -> Option<String> {
    if detection.runtime == "docker" || detection.runtime == "unknown" {
        return None;
    }

    let content = match (detection.runtime.as_str(), detection.framework.as_deref()) {
        ("nodejs", Some("Next.js")) => template_node_nextjs(project_path),
        ("nodejs", Some("Vite")) => template_node_vite(project_path),
        ("nodejs", Some("Express") | Some("Fastify")) => template_node_express(project_path),
        ("nodejs", _) => template_node_express(project_path),
        ("python", Some("FastAPI")) => template_python_fastapi(project_path),
        ("python", Some("Flask")) => template_python_flask(project_path),
        ("python", Some("Django")) => template_python_django(project_path),
        ("python", _) => template_python_fastapi(project_path),
        ("go", _) => template_go(project_path),
        ("static", _) => template_static(),
        _ => return None,
    };

    Some(content)
}

fn template_node_nextjs(path: &Path) -> String {
    let pm = detect_package_manager(path);
    let start = pm.start_cmd();
    let start_json: Vec<String> = start.iter().map(|s| format!("\"{s}\"")).collect();

    format!(
        r#"FROM node:22-slim AS deps
WORKDIR /app
{copy}
{install}

FROM node:22-slim AS build
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
{build}

FROM node:22-slim
WORKDIR /app
COPY --from=build /app/.next ./.next
COPY --from=build /app/node_modules ./node_modules
COPY --from=build /app/package.json ./
EXPOSE $PORT
CMD [{start}]
"#,
        copy = pm.copy_line(),
        install = pm.install_cmd(),
        build = pm.run_build(),
        start = start_json.join(", "),
    )
}

fn template_node_vite(path: &Path) -> String {
    let pm = detect_package_manager(path);

    format!(
        r#"FROM node:22-slim AS build
WORKDIR /app
{copy}
{install}
COPY . .
{build}

FROM node:22-slim
WORKDIR /app
RUN npm install -g serve
COPY --from=build /app/dist ./dist
EXPOSE $PORT
CMD ["sh", "-c", "serve -s dist -l $PORT"]
"#,
        copy = pm.copy_line(),
        install = pm.install_cmd(),
        build = pm.run_build(),
    )
}

fn template_node_express(path: &Path) -> String {
    let pm = detect_package_manager(path);
    let entry = detect_node_entry_point(path);

    format!(
        r#"FROM node:22-slim AS deps
WORKDIR /app
{copy}
{install}

FROM node:22-slim
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
EXPOSE $PORT
CMD ["node", "{entry}"]
"#,
        copy = pm.copy_line(),
        install = pm.install_cmd(),
    )
}

fn template_python_fastapi(path: &Path) -> String {
    let py_version = detect_python_version(path);
    let entry = detect_python_entry_point(path);

    format!(
        r#"FROM python:{py_version}-slim AS deps
WORKDIR /app
COPY pyproject.toml requirements*.txt* ./
RUN pip install --no-cache-dir -r requirements.txt 2>/dev/null || pip install --no-cache-dir .

FROM python:{py_version}-slim
WORKDIR /app
COPY --from=deps /usr/local/lib/python{py_version}/site-packages /usr/local/lib/python{py_version}/site-packages
COPY --from=deps /usr/local/bin /usr/local/bin
COPY . .
EXPOSE $PORT
CMD ["sh", "-c", "uvicorn {entry} --host 0.0.0.0 --port $PORT"]
"#,
    )
}

fn template_python_flask(path: &Path) -> String {
    let py_version = detect_python_version(path);
    let entry = detect_python_entry_point(path);
    // For gunicorn, convert "app.main:app" format to module:app
    let gunicorn_entry = entry.clone();

    format!(
        r#"FROM python:{py_version}-slim AS deps
WORKDIR /app
COPY pyproject.toml requirements*.txt* ./
RUN pip install --no-cache-dir -r requirements.txt 2>/dev/null || pip install --no-cache-dir .

FROM python:{py_version}-slim
WORKDIR /app
COPY --from=deps /usr/local/lib/python{py_version}/site-packages /usr/local/lib/python{py_version}/site-packages
COPY --from=deps /usr/local/bin /usr/local/bin
COPY . .
EXPOSE $PORT
CMD ["sh", "-c", "gunicorn {gunicorn_entry} --bind 0.0.0.0:$PORT"]
"#,
    )
}

fn template_python_django(path: &Path) -> String {
    let py_version = detect_python_version(path);

    // For Django, look for wsgi.py to find the module name
    let wsgi_module = detect_django_wsgi(path);

    format!(
        r#"FROM python:{py_version}-slim AS deps
WORKDIR /app
COPY pyproject.toml requirements*.txt* ./
RUN pip install --no-cache-dir -r requirements.txt 2>/dev/null || pip install --no-cache-dir .

FROM python:{py_version}-slim
WORKDIR /app
COPY --from=deps /usr/local/lib/python{py_version}/site-packages /usr/local/lib/python{py_version}/site-packages
COPY --from=deps /usr/local/bin /usr/local/bin
COPY . .
EXPOSE $PORT
CMD ["sh", "-c", "gunicorn {wsgi_module} --bind 0.0.0.0:$PORT"]
"#,
    )
}

fn detect_django_wsgi(path: &Path) -> String {
    // Look for manage.py to find the project name, then derive wsgi module
    if let Ok(content) = std::fs::read_to_string(path.join("manage.py")) {
        for line in content.lines() {
            // Match: os.environ.setdefault('DJANGO_SETTINGS_MODULE', 'myproject.settings')
            if line.contains("DJANGO_SETTINGS_MODULE") {
                if let Some(start) = line.rfind('\'').or_else(|| line.rfind('"')) {
                    let before = &line[..start];
                    if let Some(quote_start) = before.rfind('\'').or_else(|| before.rfind('"')) {
                        let module = &line[quote_start + 1..start];
                        // Convert "myproject.settings" to "myproject.wsgi:application"
                        if let Some(project_name) = module.split('.').next() {
                            return format!("{project_name}.wsgi:application");
                        }
                    }
                }
            }
        }
    }

    // Fallback: scan for wsgi.py files
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() && entry.path().join("wsgi.py").exists() {
                if let Some(name) = entry.file_name().to_str() {
                    return format!("{name}.wsgi:application");
                }
            }
        }
    }

    "config.wsgi:application".to_string()
}

fn template_go(path: &Path) -> String {
    // Use detected Go version if available, otherwise default
    let go_version = detect_go_version(path);

    format!(
        r#"FROM golang:{go_version} AS build
WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -o server .

FROM gcr.io/distroless/static
COPY --from=build /app/server /server
EXPOSE $PORT
CMD ["/server"]
"#,
    )
}

fn detect_go_version(path: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path.join("go.mod")) {
        for line in content.lines() {
            if line.starts_with("go ") {
                if let Some(version) = line.split_once(' ').map(|(_, v)| v.trim().to_string()) {
                    return version;
                }
            }
        }
    }
    "1.23".to_string()
}

fn template_static() -> String {
    r#"FROM node:22-slim
WORKDIR /app
RUN npm install -g serve
COPY . .
EXPOSE $PORT
CMD ["sh", "-c", "serve -s . -l $PORT"]
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::DetectionResult;
    use tempfile::TempDir;

    fn make_detection(runtime: &str, framework: Option<&str>, confidence: &str) -> DetectionResult {
        DetectionResult {
            runtime: runtime.into(),
            framework: framework.map(|f| f.into()),
            version: None,
            confidence: confidence.into(),
            reason: "test".into(),
        }
    }

    // -- generate_dockerfile selection tests --

    #[test]
    fn test_returns_none_for_docker_runtime() {
        let dir = TempDir::new().unwrap();
        let det = make_detection("docker", None, "high");
        assert!(generate_dockerfile(&det, dir.path()).is_none());
    }

    #[test]
    fn test_returns_none_for_unknown_runtime() {
        let dir = TempDir::new().unwrap();
        let det = make_detection("unknown", None, "low");
        assert!(generate_dockerfile(&det, dir.path()).is_none());
    }

    #[test]
    fn test_nextjs_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"next": "^14.0.0"}}"#,
        )
        .unwrap();
        let det = make_detection("nodejs", Some("Next.js"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("node:22-slim"));
        assert!(content.contains(".next"));
        assert!(content.contains("npm ci"));
        assert!(content.contains("npm run build"));
    }

    #[test]
    fn test_nextjs_yarn() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let det = make_detection("nodejs", Some("Next.js"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("yarn.lock"));
        assert!(content.contains("yarn install --frozen-lockfile"));
        assert!(content.contains("yarn build"));
    }

    #[test]
    fn test_nextjs_pnpm() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let det = make_detection("nodejs", Some("Next.js"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("pnpm-lock.yaml"));
        assert!(content.contains("pnpm install --frozen-lockfile"));
    }

    #[test]
    fn test_vite_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let det = make_detection("nodejs", Some("Vite"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("serve"));
        assert!(content.contains("/app/dist"));
        assert!(content.contains("npm run build"));
    }

    #[test]
    fn test_express_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"main": "server.js"}"#).unwrap();
        let det = make_detection("nodejs", Some("Express"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("node"));
        assert!(content.contains("server.js"));
        assert!(!content.contains("npm run build"));
    }

    #[test]
    fn test_express_entry_point_fallback() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("app.js"), "").unwrap();
        let det = make_detection("nodejs", Some("Express"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("app.js"));
    }

    #[test]
    fn test_fastify_uses_express_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let det = make_detection("nodejs", Some("Fastify"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("node"));
    }

    #[test]
    fn test_generic_node_uses_express_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let det = make_detection("nodejs", None, "medium");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("node"));
    }

    #[test]
    fn test_fastapi_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "fastapi\nuvicorn").unwrap();
        std::fs::write(dir.path().join("app/main.py"), "").ok(); // may fail, that's ok
        let det = make_detection("python", Some("FastAPI"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("python:3.13-slim"));
        assert!(content.contains("uvicorn"));
        assert!(content.contains("pip install"));
    }

    #[test]
    fn test_fastapi_with_version() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nrequires-python = \">=3.11\"\ndependencies = [\"fastapi\"]",
        )
        .unwrap();
        let det = make_detection("python", Some("FastAPI"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("python:3.11-slim"));
    }

    #[test]
    fn test_flask_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\ngunicorn").unwrap();
        std::fs::write(dir.path().join("app.py"), "").unwrap();
        let det = make_detection("python", Some("Flask"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("gunicorn"));
        assert!(content.contains("app:app"));
    }

    #[test]
    fn test_django_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "django\ngunicorn").unwrap();
        std::fs::write(
            dir.path().join("manage.py"),
            "os.environ.setdefault('DJANGO_SETTINGS_MODULE', 'mysite.settings')",
        )
        .unwrap();
        let det = make_detection("python", Some("Django"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("gunicorn"));
        assert!(content.contains("mysite.wsgi:application"));
    }

    #[test]
    fn test_django_fallback_wsgi() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("myapp")).unwrap();
        std::fs::write(dir.path().join("myapp/wsgi.py"), "").unwrap();
        let det = make_detection("python", Some("Django"), "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("myapp.wsgi:application"));
    }

    #[test]
    fn test_go_template() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.22\n",
        )
        .unwrap();
        let det = make_detection("go", None, "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("golang:1.22"));
        assert!(content.contains("distroless/static"));
        assert!(content.contains("go build -o server"));
    }

    #[test]
    fn test_go_version_fallback() {
        let dir = TempDir::new().unwrap();
        let det = make_detection("go", None, "high");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("golang:1.23"));
    }

    #[test]
    fn test_static_template() {
        let dir = TempDir::new().unwrap();
        let det = make_detection("static", None, "low");
        let content = generate_dockerfile(&det, dir.path()).unwrap();
        assert!(content.contains("serve"));
        assert!(content.contains("node:22-slim"));
    }

    // -- helper function tests --

    #[test]
    fn test_detect_package_manager_npm() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        assert_eq!(detect_package_manager(dir.path()), PackageManager::Npm);
    }

    #[test]
    fn test_detect_package_manager_yarn() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        assert_eq!(detect_package_manager(dir.path()), PackageManager::Yarn);
    }

    #[test]
    fn test_detect_package_manager_pnpm() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(detect_package_manager(dir.path()), PackageManager::Pnpm);
    }

    #[test]
    fn test_detect_package_manager_default_npm() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_package_manager(dir.path()), PackageManager::Npm);
    }

    #[test]
    fn test_detect_node_entry_from_main() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"main": "dist/index.js"}"#,
        )
        .unwrap();
        assert_eq!(detect_node_entry_point(dir.path()), "dist/index.js");
    }

    #[test]
    fn test_detect_node_entry_from_file_scan() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("server.js"), "").unwrap();
        assert_eq!(detect_node_entry_point(dir.path()), "server.js");
    }

    #[test]
    fn test_detect_node_entry_default() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_node_entry_point(dir.path()), "src/index.js");
    }

    #[test]
    fn test_detect_python_entry_from_file_scan() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.py"), "").unwrap();
        assert_eq!(detect_python_entry_point(dir.path()), "main:app");
    }

    #[test]
    fn test_detect_python_entry_default() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_python_entry_point(dir.path()), "app.main:app");
    }

    #[test]
    fn test_detect_python_version_from_pyproject() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nrequires-python = \">=3.11\"",
        )
        .unwrap();
        assert_eq!(detect_python_version(dir.path()), "3.11");
    }

    #[test]
    fn test_detect_python_version_default() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_python_version(dir.path()), "3.13");
    }
}
