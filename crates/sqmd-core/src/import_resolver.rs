use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TsConfigPaths {
    pub aliases: HashMap<String, String>,
}

impl TsConfigPaths {
    pub fn load(project_root: &Path) -> Self {
        let tsconfig = project_root.join("tsconfig.json");
        if !tsconfig.exists() {
            return Self {
                aliases: HashMap::new(),
            };
        }
        match std::fs::read_to_string(&tsconfig) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => {
                    let mut aliases = HashMap::new();
                    if let Some(paths) = json
                        .get("compilerOptions")
                        .and_then(|o| o.get("paths"))
                        .and_then(|p| p.as_object())
                    {
                        for (pattern, targets) in paths {
                            let pattern_clean = pattern.trim_end_matches('*');
                            let target = targets
                                .as_array()
                                .and_then(|a| a.first())
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let target_clean = target.trim_end_matches('*').trim_end_matches('/');
                            if !pattern_clean.is_empty() && !target_clean.is_empty() {
                                aliases.insert(pattern_clean.to_string(), target_clean.to_string());
                            }
                        }
                    }
                    Self { aliases }
                }
                Err(_) => Self {
                    aliases: HashMap::new(),
                },
            },
            Err(_) => Self {
                aliases: HashMap::new(),
            },
        }
    }

    pub fn resolve(&self, module_path: &str) -> Option<String> {
        if self.aliases.is_empty() {
            return None;
        }
        let mut best_match: Option<(usize, String)> = None;
        for (pattern, target) in &self.aliases {
            if module_path.starts_with(pattern) {
                let remainder = &module_path[pattern.len()..];
                let resolved = if remainder.starts_with('/') {
                    format!("{}{}", target, remainder)
                } else if remainder.is_empty() {
                    target.clone()
                } else {
                    format!("{}/{}", target, remainder)
                };
                let match_len = pattern.len();
                if best_match
                    .as_ref()
                    .map_or(true, |(len, _)| match_len > *len)
                {
                    best_match = Some((match_len, resolved));
                }
            }
        }
        best_match.map(|(_, path)| path)
    }
}

#[derive(Debug, Clone)]
pub struct CargoWorkspace {
    pub workspace_members: Vec<String>,
    pub crate_names: HashMap<String, String>,
}

impl CargoWorkspace {
    pub fn load(project_root: &Path) -> Self {
        let cargo_toml = project_root.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Self {
                workspace_members: Vec::new(),
                crate_names: HashMap::new(),
            };
        }
        match std::fs::read_to_string(&cargo_toml) {
            Ok(content) => {
                let mut workspace_members = Vec::new();
                let mut crate_names = HashMap::new();

                if let Ok(value) = content.parse::<toml::Value>() {
                    if let Some(ws) = value.get("workspace") {
                        if let Some(members) = ws.get("members").and_then(|m| m.as_array()) {
                            for member in members {
                                if let Some(s) = member.as_str() {
                                    workspace_members.push(s.to_string());
                                }
                            }
                        }
                    }
                    if let Some(pkg) = value.get("package") {
                        if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
                            crate_names.insert(
                                name.to_string(),
                                project_root.to_string_lossy().to_string(),
                            );
                        }
                    }
                }

                for member in &workspace_members {
                    let member_path = project_root.join(member);
                    let member_cargo = member_path.join("Cargo.toml");
                    if member_cargo.exists() {
                        if let Ok(member_content) = std::fs::read_to_string(&member_cargo) {
                            if let Ok(member_value) = member_content.parse::<toml::Value>() {
                                if let Some(name) = member_value
                                    .get("package")
                                    .and_then(|p| p.get("name"))
                                    .and_then(|n| n.as_str())
                                {
                                    crate_names.insert(
                                        name.to_string(),
                                        member_path.to_string_lossy().to_string(),
                                    );
                                }
                            }
                        }
                    }
                }

                Self {
                    workspace_members,
                    crate_names,
                }
            }
            Err(_) => Self {
                workspace_members: Vec::new(),
                crate_names: HashMap::new(),
            },
        }
    }

    pub fn resolve_crate(&self, crate_name: &str) -> Option<String> {
        self.crate_names.get(crate_name).cloned()
    }
}

#[derive(Debug, Clone)]
pub struct GoModule {
    pub module_path: String,
}

impl GoModule {
    pub fn load(project_root: &Path) -> Self {
        let gomod = project_root.join("go.mod");
        if !gomod.exists() {
            return Self {
                module_path: String::new(),
            };
        }
        match std::fs::read_to_string(&gomod) {
            Ok(content) => {
                let module_path = content
                    .lines()
                    .find(|l| l.starts_with("module "))
                    .map(|l| l["module ".len()..].trim().to_string())
                    .unwrap_or_default();
                Self { module_path }
            }
            Err(_) => Self {
                module_path: String::new(),
            },
        }
    }

    pub fn resolve(&self, import_path: &str, project_root: &Path) -> Option<String> {
        if self.module_path.is_empty() || !import_path.starts_with(&self.module_path) {
            return None;
        }
        let remainder = &import_path[self.module_path.len()..];
        let remainder = remainder.trim_start_matches('/');
        if remainder.is_empty() {
            return None;
        }
        Some(project_root.join(remainder).to_string_lossy().to_string())
    }
}

#[derive(Debug, Clone)]
pub struct PyProject {
    pub package_name: Option<String>,
    pub package_dir: Option<String>,
}

impl PyProject {
    pub fn load(project_root: &Path) -> Self {
        let pyproject = project_root.join("pyproject.toml");
        if !pyproject.exists() {
            return Self {
                package_name: None,
                package_dir: None,
            };
        }
        match std::fs::read_to_string(&pyproject) {
            Ok(content) => {
                let (package_name, package_dir) = if let Ok(value) = content.parse::<toml::Value>()
                {
                    let name = value
                        .get("project")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string());
                    let dir = value
                        .get("tool")
                        .and_then(|t| t.get("setuptools"))
                        .and_then(|s| s.get("packages"))
                        .and_then(|p| p.get("find"))
                        .and_then(|f| f.get("where"))
                        .and_then(|w| w.as_str())
                        .map(|s| s.to_string());
                    (name, dir)
                } else {
                    (None, None)
                };
                Self {
                    package_name,
                    package_dir,
                }
            }
            Err(_) => Self {
                package_name: None,
                package_dir: None,
            },
        }
    }

    pub fn resolve(&self, module_path: &str, project_root: &Path) -> Option<String> {
        if let Some(ref name) = self.package_name {
            if module_path.starts_with(name) {
                let remainder = &module_path[name.len()..];
                let remainder = remainder.trim_start_matches('.');
                let remainder = remainder.trim_start_matches('/');
                if remainder.is_empty() {
                    return None;
                }
                let base_dir = self
                    .package_dir
                    .as_deref()
                    .map(|d| project_root.join(d))
                    .unwrap_or_else(|| project_root.join("src"));
                return Some(
                    base_dir
                        .join(remainder.replace('.', "/"))
                        .join("__init__.py")
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
        let parts: Vec<&str> = module_path.split('.').collect();
        if parts.len() >= 2 {
            let base_dir = self
                .package_dir
                .as_deref()
                .map(|d| project_root.join(d))
                .unwrap_or_else(|| project_root.join("src"));
            Some(
                base_dir
                    .join(parts.join("/"))
                    .join("__init__.py")
                    .to_string_lossy()
                    .to_string(),
            )
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsconfig_alias_resolution() {
        let json = r#"{"compilerOptions":{"paths":{"@components/*":["src/components/*"],"@lib/*":["lib/*"]}}}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut aliases = HashMap::new();
        aliases.insert("@components/".to_string(), "src/components".to_string());
        aliases.insert("@lib/".to_string(), "lib".to_string());
        let paths = TsConfigPaths { aliases };
        assert_eq!(
            paths.resolve("@components/Button"),
            Some("src/components/Button".to_string())
        );
        assert_eq!(paths.resolve("@lib/utils"), Some("lib/utils".to_string()));
        assert_eq!(paths.resolve("react"), None);
    }

    #[test]
    fn gomod_resolve() {
        let go = GoModule {
            module_path: "github.com/example/myapp".to_string(),
        };
        let root = Path::new("/tmp/project");
        assert_eq!(
            go.resolve("github.com/example/myapp/pkg/server", root),
            Some("/tmp/project/pkg/server".to_string())
        );
        assert_eq!(go.resolve("fmt", root), None);
    }

    #[test]
    fn gomod_empty() {
        let go = GoModule {
            module_path: String::new(),
        };
        assert_eq!(go.resolve("anything", Path::new("/tmp")), None);
    }

    #[test]
    fn pyproject_resolve() {
        let py = PyProject {
            package_name: Some("mypackage".to_string()),
            package_dir: None,
        };
        let root = Path::new("/tmp/project");
        assert_eq!(
            py.resolve("mypackage.utils.helpers", root),
            Some("/tmp/project/src/utils/helpers/__init__.py".to_string())
        );
    }

    #[test]
    fn cargo_workspace_load_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = CargoWorkspace::load(tmp.path());
        assert!(ws.workspace_members.is_empty());
    }
}
