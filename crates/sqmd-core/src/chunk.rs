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
    pub content_md: String,
    pub content_hash: String,
    pub metadata: Map<String, serde_json::Value>,
}
