use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectsRegistry {
    #[serde(default)]
    pub projects: HashMap<String, ProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: String,
}

impl ProjectsRegistry {
    pub fn load() -> Self {
        let path = Self::registry_path();
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(reg) => reg,
                Err(e) => {
                    eprintln!("[multi-project] failed to parse {:?}: {}", path, e);
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("[multi-project] failed to read {:?}: {}", path, e);
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::registry_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn add(&mut self, name: String, path: String) {
        self.projects.insert(name, ProjectEntry { path });
    }

    pub fn remove(&mut self, name: &str) {
        self.projects.remove(name);
    }

    pub fn resolve_path(&self, name_or_path: &str) -> Option<PathBuf> {
        if let Some(entry) = self.projects.get(name_or_path) {
            let p = PathBuf::from(&entry.path);
            if p.exists() {
                return Some(p);
            }
        }
        let p = PathBuf::from(name_or_path);
        if p.exists() && p.join(".sqmd").join("index.db").exists() {
            return Some(p);
        }
        None
    }

    pub fn list(&self) -> Vec<(&String, &ProjectEntry)> {
        let mut entries: Vec<_> = self.projects.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        entries
    }

    fn registry_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sqmd")
            .join("projects.toml")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiProjectResult {
    pub project: String,
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub line_start: i64,
    pub line_end: i64,
    pub score: f64,
    pub content_raw: String,
    pub language: String,
    pub markdown: String,
}

pub fn multi_project_search(
    project_paths: &[PathBuf],
    query: &str,
    top_k: usize,
) -> Result<Vec<MultiProjectResult>, Box<dyn std::error::Error>> {
    let mut all_results: Vec<MultiProjectResult> = Vec::new();

    for project_root in project_paths {
        let db_path = project_root.join(".sqmd").join("index.db");
        if !db_path.exists() {
            continue;
        }
        let project_name = project_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| project_root.to_string_lossy().to_string());

        let conn = match Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let search_query = crate::search::SearchQuery {
            text: query.to_string(),
            top_k,
            source_type_filter: None,
            ..Default::default()
        };

        let results = crate::search::fts_search(&conn, &search_query)?;

        for r in results {
            let content_raw = get_chunk_content(&conn, r.chunk_id).unwrap_or_default();
            let markdown = render_markdown(
                &r.file_path,
                &r.name,
                &r.chunk_type,
                r.line_start,
                r.line_end,
                &content_raw,
                "",
            );
            all_results.push(MultiProjectResult {
                project: project_name.clone(),
                file_path: r.file_path,
                name: r.name,
                chunk_type: r.chunk_type,
                line_start: r.line_start,
                line_end: r.line_end,
                score: r.score,
                content_raw,
                language: String::new(),
                markdown,
            });
        }
    }

    all_results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_results.truncate(top_k);
    Ok(all_results)
}

fn get_chunk_content(db: &Connection, chunk_id: i64) -> Result<String, Box<dyn std::error::Error>> {
    let result: String = db
        .query_row(
            "SELECT content_raw FROM chunks WHERE id = ?1",
            rusqlite::params![chunk_id],
            |r| r.get(0),
        )
        .unwrap_or_default();
    Ok(result)
}

fn render_markdown(
    file_path: &str,
    name: &Option<String>,
    chunk_type: &str,
    line_start: i64,
    line_end: i64,
    content: &str,
    language: &str,
) -> String {
    let name = name.as_deref().unwrap_or("(unnamed)");
    format!(
        "### `{}`\n\n**File:** `{}` | **Lines:** {}-{} | **Type:** {}\n\n```{}\n{}\n```\n",
        name,
        file_path,
        line_start + 1,
        line_end + 1,
        chunk_type,
        language,
        content.trim(),
    )
}

pub fn multi_project_stats(project_paths: &[PathBuf]) -> Vec<(String, Option<MultiProjectStats>)> {
    let mut results = Vec::new();
    for project_root in project_paths {
        let db_path = project_root.join(".sqmd").join("index.db");
        let project_name = project_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| project_root.to_string_lossy().to_string());

        if !db_path.exists() {
            results.push((project_name, None));
            continue;
        }

        let conn = match Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(c) => c,
            Err(_) => {
                results.push((project_name, None));
                continue;
            }
        };

        let stats = match get_project_stats(&conn) {
            Ok(s) => Some(s),
            Err(_) => None,
        };
        results.push((project_name, stats));
    }
    results
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiProjectStats {
    pub chunks: i64,
    pub files: i64,
    pub relationships: i64,
    pub entities: i64,
}

fn get_project_stats(db: &Connection) -> Result<MultiProjectStats, Box<dyn std::error::Error>> {
    Ok(MultiProjectStats {
        chunks: db.query_row(
            "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )?,
        files: db.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?,
        relationships: db.query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))?,
        entities: db.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_load_missing() {
        let reg = ProjectsRegistry::load();
        assert!(reg.projects.is_empty());
    }

    #[test]
    fn registry_add_and_resolve() {
        let mut reg = ProjectsRegistry::default();
        reg.add("test-proj".to_string(), "/tmp/nonexistent".to_string());
        assert!(reg.projects.contains_key("test-proj"));
        let resolved = reg.resolve_path("/tmp/nonexistent");
        assert!(resolved.is_none());
    }

    #[test]
    fn registry_remove() {
        let mut reg = ProjectsRegistry::default();
        reg.add("test-proj".to_string(), "/tmp/test".to_string());
        reg.remove("test-proj");
        assert!(!reg.projects.contains_key("test-proj"));
    }

    #[test]
    fn registry_list_sorted() {
        let mut reg = ProjectsRegistry::default();
        reg.add("zebra".to_string(), "/z".to_string());
        reg.add("alpha".to_string(), "/a".to_string());
        let list = reg.list();
        assert_eq!(list[0].0, &"alpha".to_string());
        assert_eq!(list[1].0, &"zebra".to_string());
    }
}
