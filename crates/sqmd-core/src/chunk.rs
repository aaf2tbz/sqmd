use serde_json::Map;

// ── Source type discriminator ───────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SourceType {
    #[default]
    Code,
    Memory,
    Transcript,
    Document,
    Entity,
}

impl SourceType {
    pub fn as_str(&self) -> &str {
        match self {
            SourceType::Code => "code",
            SourceType::Memory => "memory",
            SourceType::Transcript => "transcript",
            SourceType::Document => "document",
            SourceType::Entity => "entity",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "code" => Some(SourceType::Code),
            "memory" => Some(SourceType::Memory),
            "transcript" => Some(SourceType::Transcript),
            "document" => Some(SourceType::Document),
            "entity" => Some(SourceType::Entity),
            _ => None,
        }
    }
}

// ── Chunk types ─────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkType {
    // Code types (existing)
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
    // Knowledge types (new)
    Fact,
    Summary,
    EntityDescription,
    DocumentSection,
    Preference,
    Decision,
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
            ChunkType::Fact => "fact",
            ChunkType::Summary => "summary",
            ChunkType::EntityDescription => "entity_description",
            ChunkType::DocumentSection => "document_section",
            ChunkType::Preference => "preference",
            ChunkType::Decision => "decision",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "function" => Some(ChunkType::Function),
            "method" => Some(ChunkType::Method),
            "class" => Some(ChunkType::Class),
            "interface" => Some(ChunkType::Interface),
            "type" => Some(ChunkType::Type),
            "module" => Some(ChunkType::Module),
            "section" => Some(ChunkType::Section),
            "import" => Some(ChunkType::Import),
            "export" => Some(ChunkType::Export),
            "macro" => Some(ChunkType::Macro),
            "trait" => Some(ChunkType::Trait),
            "impl" => Some(ChunkType::Impl),
            "enum" => Some(ChunkType::Enum),
            "struct" => Some(ChunkType::Struct),
            "constant" => Some(ChunkType::Constant),
            "fact" => Some(ChunkType::Fact),
            "summary" => Some(ChunkType::Summary),
            "entity_description" => Some(ChunkType::EntityDescription),
            "document_section" => Some(ChunkType::DocumentSection),
            "preference" => Some(ChunkType::Preference),
            "decision" => Some(ChunkType::Decision),
            _ => None,
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
            // Knowledge types — importance is typically set explicitly
            ChunkType::Fact => 0.7,
            ChunkType::Decision => 0.8,
            ChunkType::Preference => 0.75,
            ChunkType::Summary => 0.6,
            ChunkType::EntityDescription => 0.65,
            ChunkType::DocumentSection => 0.5,
        }
    }

    pub fn is_code(&self) -> bool {
        matches!(
            self,
            ChunkType::Function | ChunkType::Method | ChunkType::Class
                | ChunkType::Interface | ChunkType::Type | ChunkType::Module
                | ChunkType::Section | ChunkType::Import | ChunkType::Export
                | ChunkType::Macro | ChunkType::Trait | ChunkType::Impl
                | ChunkType::Enum | ChunkType::Struct | ChunkType::Constant
        )
    }

    pub fn is_knowledge(&self) -> bool {
        !self.is_code()
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub file_path: String,
    pub language: String,
    pub chunk_type: ChunkType,
    pub source_type: SourceType,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub content_raw: String,
    pub content_hash: String,
    pub importance: f64,
    pub metadata: Map<String, serde_json::Value>,
    // Knowledge-specific fields
    pub agent_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub decay_rate: f64,
    pub created_by: Option<String>,
}

impl Chunk {
    /// Create a knowledge chunk (memory, fact, etc.) without code-specific fields.
    pub fn knowledge(
        chunk_type: ChunkType,
        source_type: SourceType,
        name: Option<String>,
        content: String,
        importance: f64,
    ) -> Self {
        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            content.hash(&mut h);
            format!("{:016x}", h.finish())
        };
        Self {
            file_path: format!("signet://{}", source_type.as_str()),
            language: String::new(),
            chunk_type,
            source_type,
            name,
            signature: None,
            line_start: 0,
            line_end: 0,
            content_raw: content,
            content_hash: hash,
            importance,
            metadata: Map::new(),
            agent_id: None,
            tags: None,
            decay_rate: 0.0,
            created_by: None,
        }
    }

    pub fn render_md(&self) -> String {
        if self.source_type != SourceType::Code {
            return self.render_knowledge_md();
        }
        let sig_line = self.signature.as_deref()
            .map(|s| format!("\n**Signature:** `{}`", s))
            .unwrap_or_default();
        format!(
            "<document>\n<source>{}</source>\n<location>{}:{}</location>\n<lines>{}</lines>\n<type>{}</type>\n<importance>{:.2}</importance>\n</document>\n\n### `{}`{}\n\n**File:** `{}` | **Lines:** {}-{} | **Type:** {}\n\n```{}\n{}\n```",
            self.file_path,
            self.file_path,
            self.line_start + 1,
            self.line_end + 1,
            self.chunk_type.as_str(),
            self.importance,
            self.name.as_deref().unwrap_or("(unnamed)"),
            sig_line,
            self.file_path,
            self.line_start + 1,
            self.line_end + 1,
            self.chunk_type.as_str(),
            self.language,
            self.content_raw,
        )
    }

    fn render_knowledge_md(&self) -> String {
        let tags_line = self.tags.as_ref()
            .map(|t| format!("\n**Tags:** {}", t.join(", ")))
            .unwrap_or_default();
        format!(
            "<memory>\n<type>{}</type>\n<source>{}</source>\n<importance>{:.2}</importance>\n</memory>\n\n### {}{}\n\n{}\n",
            self.chunk_type.as_str(),
            self.source_type.as_str(),
            self.importance,
            self.name.as_deref().unwrap_or("(memory)"),
            tags_line,
            self.content_raw,
        )
    }
}
