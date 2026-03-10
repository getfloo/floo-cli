use std::path::Path;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct DetectionResult {
    pub runtime: String,
    pub framework: Option<String>,
    pub version: Option<String>,
    pub confidence: String,
    pub reason: String,
}

impl DetectionResult {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn default_port(&self) -> u16 {
        match self.runtime.as_str() {
            "nodejs" => 3000,
            "python" => 8000,
            "go" => 8080,
            "docker" => 8080,
            "static" => 8080,
            _ => 8080,
        }
    }

    pub fn default_service_type(&self) -> &str {
        match self.framework.as_deref() {
            Some("Express") | Some("Fastify") | Some("FastAPI") | Some("Flask")
            | Some("Django") => "api",
            _ => "web",
        }
    }
}

fn detect_dockerfile(path: &Path) -> Option<DetectionResult> {
    if path.join("Dockerfile").exists() {
        Some(DetectionResult {
            runtime: "docker".into(),
            framework: None,
            version: None,
            confidence: "high".into(),
            reason: "Dockerfile found".into(),
        })
    } else {
        None
    }
}

fn detect_nodejs(path: &Path) -> Option<DetectionResult> {
    let pkg_path = path.join("package.json");
    if !pkg_path.exists() {
        return None;
    }

    let content = match std::fs::read_to_string(&pkg_path) {
        Ok(c) => c,
        Err(_) => {
            return Some(DetectionResult {
                runtime: "nodejs".into(),
                framework: None,
                version: None,
                confidence: "medium".into(),
                reason: "package.json found but could not be parsed".into(),
            });
        }
    };

    let pkg: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => {
            return Some(DetectionResult {
                runtime: "nodejs".into(),
                framework: None,
                version: None,
                confidence: "medium".into(),
                reason: "package.json found but could not be parsed".into(),
            });
        }
    };

    // Merge dependencies and devDependencies
    let mut deps = serde_json::Map::new();
    if let Some(d) = pkg.get("dependencies").and_then(|v| v.as_object()) {
        deps.extend(d.clone());
    }
    if let Some(d) = pkg.get("devDependencies").and_then(|v| v.as_object()) {
        deps.extend(d.clone());
    }

    let frameworks = [
        ("next", "Next.js"),
        ("vite", "Vite"),
        ("express", "Express"),
        ("fastify", "Fastify"),
    ];

    for (dep_name, framework_name) in &frameworks {
        if let Some(version_val) = deps.get(*dep_name) {
            return Some(DetectionResult {
                runtime: "nodejs".into(),
                framework: Some(framework_name.to_string()),
                version: version_val.as_str().map(|s| s.to_string()),
                confidence: "high".into(),
                reason: format!("package.json contains {dep_name} dependency"),
            });
        }
    }

    Some(DetectionResult {
        runtime: "nodejs".into(),
        framework: None,
        version: None,
        confidence: "medium".into(),
        reason: "package.json found".into(),
    })
}

fn detect_python(path: &Path) -> Option<DetectionResult> {
    let pyproject_path = path.join("pyproject.toml");
    if pyproject_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&pyproject_path) {
            let lower = content.to_lowercase();
            let frameworks = [
                ("fastapi", "FastAPI"),
                ("flask", "Flask"),
                ("django", "Django"),
            ];
            for (dep_name, framework_name) in &frameworks {
                if lower.contains(dep_name) {
                    return Some(DetectionResult {
                        runtime: "python".into(),
                        framework: Some(framework_name.to_string()),
                        version: None,
                        confidence: "high".into(),
                        reason: format!("pyproject.toml references {dep_name}"),
                    });
                }
            }
            return Some(DetectionResult {
                runtime: "python".into(),
                framework: None,
                version: None,
                confidence: "medium".into(),
                reason: "pyproject.toml found".into(),
            });
        }
    }

    let req_path = path.join("requirements.txt");
    if req_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&req_path) {
            let lower = content.to_lowercase();
            let frameworks = [
                ("fastapi", "FastAPI"),
                ("flask", "Flask"),
                ("django", "Django"),
            ];
            for (dep_name, framework_name) in &frameworks {
                if lower.contains(dep_name) {
                    return Some(DetectionResult {
                        runtime: "python".into(),
                        framework: Some(framework_name.to_string()),
                        version: None,
                        confidence: "high".into(),
                        reason: format!("requirements.txt contains {dep_name}"),
                    });
                }
            }
            return Some(DetectionResult {
                runtime: "python".into(),
                framework: None,
                version: None,
                confidence: "medium".into(),
                reason: "requirements.txt found".into(),
            });
        }
    }

    None
}

fn detect_go(path: &Path) -> Option<DetectionResult> {
    let gomod_path = path.join("go.mod");
    if !gomod_path.exists() {
        return None;
    }

    let mut version = None;
    if let Ok(content) = std::fs::read_to_string(&gomod_path) {
        for line in content.lines() {
            if line.starts_with("go ") {
                version = line.split_once(' ').map(|(_, v)| v.trim().to_string());
                break;
            }
        }
    }

    Some(DetectionResult {
        runtime: "go".into(),
        framework: None,
        version,
        confidence: "high".into(),
        reason: "go.mod found".into(),
    })
}

fn detect_static(path: &Path) -> Option<DetectionResult> {
    if path.join("index.html").exists() {
        Some(DetectionResult {
            runtime: "static".into(),
            framework: None,
            version: None,
            confidence: "low".into(),
            reason: "index.html found with no backend markers".into(),
        })
    } else {
        None
    }
}

/// Run detection per-service subdirectory. Returns the primary detection (first known runtime)
/// and per-service results as (service_name, detection) pairs.
pub fn detect_for_services(
    config_dir: &Path,
    services: &[(&str, &str)], // (name, path) pairs
) -> (DetectionResult, Vec<(String, DetectionResult)>) {
    let mut primary: Option<DetectionResult> = None;
    let mut per_service = Vec::new();

    for &(name, svc_path) in services {
        let dir = if svc_path == "." {
            config_dir.to_path_buf()
        } else {
            config_dir.join(svc_path)
        };
        let result = detect(&dir);
        if primary.is_none() && result.runtime != "unknown" {
            primary = Some(result.clone());
        }
        per_service.push((name.to_string(), result));
    }

    let primary = primary.unwrap_or_else(|| DetectionResult {
        runtime: "unknown".into(),
        framework: None,
        version: None,
        confidence: "low".into(),
        reason: "No recognized project files found in any service".into(),
    });

    (primary, per_service)
}

pub fn detect(path: &Path) -> DetectionResult {
    let detectors: Vec<fn(&Path) -> Option<DetectionResult>> = vec![
        detect_dockerfile,
        detect_nodejs,
        detect_python,
        detect_go,
        detect_static,
    ];

    for detector in detectors {
        if let Some(result) = detector(path) {
            return result;
        }
    }

    DetectionResult {
        runtime: "unknown".into(),
        framework: None,
        version: None,
        confidence: "low".into(),
        reason: "No recognized project files found".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_dockerfile() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Dockerfile"), "FROM node:18").unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "docker");
        assert_eq!(result.confidence, "high");
    }

    #[test]
    fn test_detect_nodejs_nextjs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"next": "^14.0.0"}}"#,
        )
        .unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "nodejs");
        assert_eq!(result.framework.as_deref(), Some("Next.js"));
        assert_eq!(result.confidence, "high");
    }

    #[test]
    fn test_detect_nodejs_vite() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vite": "^5.0.0"}}"#,
        )
        .unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "nodejs");
        assert_eq!(result.framework.as_deref(), Some("Vite"));
    }

    #[test]
    fn test_detect_nodejs_express() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"express": "^4.18.0"}}"#,
        )
        .unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "nodejs");
        assert_eq!(result.framework.as_deref(), Some("Express"));
    }

    #[test]
    fn test_detect_python_fastapi() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("requirements.txt"),
            "fastapi>=0.100\nuvicorn",
        )
        .unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "python");
        assert_eq!(result.framework.as_deref(), Some("FastAPI"));
        assert_eq!(result.confidence, "high");
    }

    #[test]
    fn test_detect_python_flask() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\ngunicorn").unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "python");
        assert_eq!(result.framework.as_deref(), Some("Flask"));
    }

    #[test]
    fn test_detect_python_django_pyproject() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\ndependencies = [\"django>=4.0\"]",
        )
        .unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "python");
        assert_eq!(result.framework.as_deref(), Some("Django"));
    }

    #[test]
    fn test_detect_go() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/app\n\ngo 1.22\n",
        )
        .unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "go");
        assert_eq!(result.version.as_deref(), Some("1.22"));
        assert_eq!(result.confidence, "high");
    }

    #[test]
    fn test_detect_static_html() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "static");
        assert_eq!(result.confidence, "low");
    }

    #[test]
    fn test_detect_unknown() {
        let dir = TempDir::new().unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "unknown");
        assert_eq!(result.confidence, "low");
    }

    #[test]
    fn test_dockerfile_takes_priority() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Dockerfile"), "FROM python:3.12").unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"next": "^14.0.0"}}"#,
        )
        .unwrap();
        let result = detect(dir.path());
        assert_eq!(result.runtime, "docker");
    }

    #[test]
    fn test_default_port_nodejs() {
        let result = DetectionResult {
            runtime: "nodejs".into(),
            framework: None,
            version: None,
            confidence: "high".into(),
            reason: "test".into(),
        };
        assert_eq!(result.default_port(), 3000);
    }

    #[test]
    fn test_default_port_python() {
        let result = DetectionResult {
            runtime: "python".into(),
            framework: None,
            version: None,
            confidence: "high".into(),
            reason: "test".into(),
        };
        assert_eq!(result.default_port(), 8000);
    }

    #[test]
    fn test_default_port_go() {
        let result = DetectionResult {
            runtime: "go".into(),
            framework: None,
            version: None,
            confidence: "high".into(),
            reason: "test".into(),
        };
        assert_eq!(result.default_port(), 8080);
    }

    #[test]
    fn test_default_service_type_express_is_api() {
        let result = DetectionResult {
            runtime: "nodejs".into(),
            framework: Some("Express".into()),
            version: None,
            confidence: "high".into(),
            reason: "test".into(),
        };
        assert_eq!(result.default_service_type(), "api");
    }

    #[test]
    fn test_default_service_type_nextjs_is_web() {
        let result = DetectionResult {
            runtime: "nodejs".into(),
            framework: Some("Next.js".into()),
            version: None,
            confidence: "high".into(),
            reason: "test".into(),
        };
        assert_eq!(result.default_service_type(), "web");
    }

    #[test]
    fn test_default_service_type_no_framework_is_web() {
        let result = DetectionResult {
            runtime: "nodejs".into(),
            framework: None,
            version: None,
            confidence: "high".into(),
            reason: "test".into(),
        };
        assert_eq!(result.default_service_type(), "web");
    }

    #[test]
    fn test_detection_result_to_value() {
        let result = DetectionResult {
            runtime: "nodejs".into(),
            framework: Some("Express".into()),
            version: Some("^4.18.0".into()),
            confidence: "high".into(),
            reason: "package.json contains express dependency".into(),
        };
        let val = result.to_value();
        assert_eq!(val["runtime"], "nodejs");
        assert_eq!(val["framework"], "Express");
    }
}
