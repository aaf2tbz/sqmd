use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Language {
    TypeScript,
    TSX,
    JavaScript,
    JSX,
    Rust,
    Python,
    Go,
    Java,
    C,
    Cpp,
    CMake,
    Qml,
    Meson,
    Ruby,
    Markdown,
    Json,
    Yaml,
    Toml,
    Html,
    Css,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "ts" => Language::TypeScript,
            "tsx" => Language::TSX,
            "js" => Language::JavaScript,
            "jsx" => Language::JSX,
            "rs" => Language::Rust,
            "py" => Language::Python,
            "go" => Language::Go,
            "java" => Language::Java,
            "c" | "h" => Language::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Language::Cpp,
            "qml" => Language::Qml,
            "rb" => Language::Ruby,
            "md" | "mdx" => Language::Markdown,
            "json" | "jsonc" => Language::Json,
            "yml" | "yaml" => Language::Yaml,
            "toml" => Language::Toml,
            "html" | "htm" => Language::Html,
            "css" | "scss" | "sass" | "less" => Language::Css,
            _ => Language::Unknown,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Language::TypeScript => "typescript",
            Language::TSX => "tsx",
            Language::JavaScript => "javascript",
            Language::JSX => "jsx",
            Language::Rust => "rust",
            Language::Python => "python",
            Language::Go => "go",
            Language::Java => "java",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::CMake => "cmake",
            Language::Qml => "qml",
            Language::Meson => "meson",
            Language::Ruby => "ruby",
            Language::Markdown => "markdown",
            Language::Json => "json",
            Language::Yaml => "yaml",
            Language::Toml => "toml",
            Language::Html => "html",
            Language::Css => "css",
            Language::Unknown => "unknown",
        }
    }

    pub fn supported(&self) -> bool {
        !matches!(self, Language::Unknown)
    }
}

pub fn detect_language(path: &Path) -> Language {
    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match fname {
        "CMakeLists.txt" => return Language::CMake,
        "meson.build" | "meson_options.txt" => return Language::Meson,
        _ => {}
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext == "cmake" {
            return Language::CMake;
        }
        return Language::from_extension(ext);
    }
    Language::Unknown
}

pub fn content_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language: Language,
    pub size: u64,
    pub mtime: f64,
    pub hash: String,
}

impl SourceFile {
    pub fn from_path(path: &Path) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let content = std::fs::read(path)?;
        let language = detect_language(path);
        let hash = content_hash(&content);
        Ok(Self {
            path: path.to_path_buf(),
            language,
            size,
            mtime,
            hash,
        })
    }
}

pub fn walk_project(root: &Path) -> impl Iterator<Item = PathBuf> {
    let mut builder = ignore::WalkBuilder::new(root);
    let builder = builder
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);
    builder.filter_entry(|entry| {
        let name = entry.file_name().to_string_lossy();
        if name == ".git"
            || name == "node_modules"
            || name == "target"
            || name == ".sqmd"
            || name == "dist"
            || name == "build"
            || name == "__pycache__"
            || name == ".venv"
            || name == "vendor"
            || name == ".next"
            || name == ".nuxt"
            || name == "coverage"
        {
            return false;
        }
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            let lang = detect_language(entry.path());
            return lang.supported();
        }
        true
    });

    builder.build().filter_map(|entry| {
        let entry = entry.ok()?;
        if entry.file_type()?.is_file() {
            Some(entry.path().to_path_buf())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(
            detect_language(Path::new("src/main.ts")),
            Language::TypeScript
        );
        assert_eq!(detect_language(Path::new("src/App.tsx")), Language::TSX);
        assert_eq!(detect_language(Path::new("src/main.rs")), Language::Rust);
        assert_eq!(detect_language(Path::new("src/main.py")), Language::Python);
        assert_eq!(detect_language(Path::new("README.md")), Language::Markdown);
        assert_eq!(detect_language(Path::new("Cargo.toml")), Language::Toml);
        assert_eq!(detect_language(Path::new("data.xyz")), Language::Unknown);
    }

    #[test]
    fn test_content_hash_deterministic() {
        let h1 = content_hash(b"hello world");
        let h2 = content_hash(b"hello world");
        let h3 = content_hash(b"hello worle");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_walk_project() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let root = PathBuf::from(dir)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let files: Vec<_> = walk_project(&root).collect();
        assert!(!files.is_empty());
        for f in &files {
            assert!(detect_language(f).supported());
        }
    }
}
