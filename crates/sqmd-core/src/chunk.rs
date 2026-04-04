use serde_json::Map;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkType {
    Function,
    Method,
    Class,
    Interface,
    Type,
    Module,
    Section,
    Import,
    Export,
    Macro,
    Trait,
    Impl,
    Enum,
    Struct,
    Constant,
}

impl ChunkType {
    pub fn as_str(&self) -> &str {
        match self {
            ChunkType::Function => "function",
            ChunkType::Method => "method",
            ChunkType::Class => "class",
            ChunkType::Interface => "interface",
            ChunkType::Type => "type",
            ChunkType::Module => "module",
            ChunkType::Section => "section",
            ChunkType::Import => "import",
            ChunkType::Export => "export",
            ChunkType::Macro => "macro",
            ChunkType::Trait => "trait",
            ChunkType::Impl => "impl",
            ChunkType::Enum => "enum",
            ChunkType::Struct => "struct",
            ChunkType::Constant => "constant",
        }
    }

    pub fn importance(&self) -> f64 {
        match self {
            ChunkType::Function | ChunkType::Method => 0.9,
            ChunkType::Class | ChunkType::Interface | ChunkType::Trait => 0.85,
            ChunkType::Impl | ChunkType::Enum | ChunkType::Struct => 0.8,
            ChunkType::Type | ChunkType::Macro => 0.7,
            ChunkType::Constant | ChunkType::Export => 0.6,
            ChunkType::Module => 0.5,
            ChunkType::Import => 0.3,
            ChunkType::Section => 0.2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub file_path: String,
    pub language: String,
    pub chunk_type: ChunkType,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub content_raw: String,
    pub content_hash: String,
    pub importance: f64,
    pub metadata: Map<String, serde_json::Value>,
}

impl Chunk {
    pub fn render_md(&self) -> String {
        let name = self.name.as_deref().unwrap_or("(unnamed)");
        let sig = self.signature.as_deref()
            .map(|s| format!("\n**Signature:** `{}`", s))
            .unwrap_or_default();
        format!(
            "### `{}`{}\n\n**File:** `{}`\n**Lines:** {}-{}\n**Type:** {}\n\n```{}\n{}\n```",
            name,
            sig,
            self.file_path,
            self.line_start + 1,
            self.line_end + 1,
            self.chunk_type.as_str(),
            self.language,
            self.content_raw,
        )
    }
}
