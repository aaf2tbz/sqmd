use crate::chunk::ChunkType;

pub struct MesonChunker;

impl MesonChunker {
    pub fn chunk(source: &str, file_path: &str) -> Vec<crate::chunk::Chunk> {
        let mut chunks = Vec::new();
        let lines: Vec<&str> = source.lines().collect();
        let mut current_start = 0usize;
        let mut current_lines: Vec<&str> = Vec::new();
        let mut brace_depth = 0i32;
        let mut paren_depth = 0i32;
        let mut current_kind: Option<ChunkType> = None;
        let mut current_name: Option<String> = None;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_num = i + 1;

            if trimmed.is_empty() && current_lines.is_empty() {
                current_start = line_num;
                continue;
            }

            let is_declaration = current_lines.is_empty()
                && (trimmed.starts_with("project(")
                    || trimmed.starts_with("executable(")
                    || trimmed.starts_with("library(")
                    || trimmed.starts_with("shared_library(")
                    || trimmed.starts_with("static_library(")
                    || trimmed.starts_with("both_libraries(")
                    || trimmed.starts_with("dependency(")
                    || trimmed.starts_with("find_library(")
                    || trimmed.starts_with("subdir(")
                    || trimmed.starts_with("import(")
                    || trimmed.starts_with("custom_target(")
                    || trimmed.starts_with("run_target(")
                    || trimmed.starts_with("benchmark(")
                    || trimmed.starts_with("test(")
                    || trimmed.starts_with("install_data(")
                    || trimmed.starts_with("configure_file(")
                    || trimmed.starts_with("generator(")
                    || trimmed.starts_with("files(")
                    || is_meson_assignment(trimmed));

            if is_declaration {
                current_start = line_num;
                current_kind = Some(classify_meson_declaration(trimmed));
                current_name = extract_meson_name(trimmed);
                current_lines.push(trimmed);
                brace_depth = count_chars(trimmed, '(') as i32 - count_chars(trimmed, ')') as i32;
                paren_depth = brace_depth;
            } else if !current_lines.is_empty() {
                current_lines.push(trimmed);
                paren_depth += count_chars(trimmed, '(') as i32 - count_chars(trimmed, ')') as i32;
                brace_depth += count_chars(trimmed, '{') as i32 - count_chars(trimmed, '}') as i32;
            } else {
                current_start = line_num;
                current_lines.push(trimmed);
                current_kind = None;
                paren_depth = count_chars(trimmed, '(') as i32 - count_chars(trimmed, ')') as i32;
            }

            let balanced = paren_depth <= 0 && brace_depth <= 0;
            let too_long = current_lines.len() >= 50;

            if (balanced || too_long) && !current_lines.is_empty() {
                let content = current_lines.join("\n");
                let sig = current_lines
                    .first()
                    .map(|l| {
                        let s = l.trim();
                        if s.len() > 120 {
                            &s[..120]
                        } else {
                            s
                        }
                    })
                    .map(|s| s.to_string());

                chunks.push(crate::chunk::Chunk {
                    file_path: file_path.to_string(),
                    language: "meson".to_string(),
                    chunk_type: current_kind.unwrap_or(ChunkType::Section),
                    source_type: crate::chunk::SourceType::Code,
                    name: current_name.take(),
                    signature: sig,
                    line_start: current_start,
                    line_end: line_num,
                    content_raw: content,
                    content_hash: String::new(),
                    importance: current_kind.map_or(0.2, |k| match k {
                        ChunkType::Function => 0.9,
                        ChunkType::Import => 0.5,
                        ChunkType::Module => 0.85,
                        ChunkType::Constant => 0.6,
                        _ => 0.2,
                    }),
                    metadata: serde_json::Map::new(),
                    agent_id: None,
                    tags: None,
                    decay_rate: 0.0,
                    created_by: None,
                });
                current_lines.clear();
                current_kind = None;
                current_name = None;
                paren_depth = 0;
                brace_depth = 0;
            }
        }

        if !current_lines.is_empty() {
            let content = current_lines.join("\n");
            chunks.push(crate::chunk::Chunk {
                file_path: file_path.to_string(),
                language: "meson".to_string(),
                chunk_type: current_kind.unwrap_or(ChunkType::Section),
                source_type: crate::chunk::SourceType::Code,
                name: current_name,
                signature: current_lines.first().map(|s| s.to_string()),
                line_start: current_start,
                line_end: lines.len(),
                content_raw: content,
                content_hash: String::new(),
                importance: 0.2,
                metadata: serde_json::Map::new(),
                agent_id: None,
                tags: None,
                decay_rate: 0.0,
                created_by: None,
            });
        }

        chunks
    }
}

fn classify_meson_declaration(line: &str) -> ChunkType {
    let lower = line.to_lowercase();
    if lower.contains("executable(")
        || lower.contains("library(")
        || lower.contains("shared_library(")
        || lower.contains("static_library(")
        || lower.contains("both_libraries(")
    {
        ChunkType::Module
    } else if lower.contains("dependency(")
        || lower.contains("find_library(")
        || lower.starts_with("subdir(")
        || lower.starts_with("import(")
    {
        ChunkType::Import
    } else if lower.starts_with("project(") {
        ChunkType::Module
    } else if lower.contains("custom_target(")
        || lower.contains("run_target(")
        || lower.starts_with("test(")
        || lower.starts_with("benchmark(")
    {
        ChunkType::Function
    } else {
        ChunkType::Section
    }
}

fn extract_meson_name(line: &str) -> Option<String> {
    let re = regex::Regex::new(r#"(?:executable|library|shared_library|static_library|both_libraries|custom_target|run_target|test|benchmark|subdir|dependency)\s*\(\s*'([^']*)'|\"([^\"]*)\""#).ok()?;
    let caps = re.captures(line)?;
    caps.get(1)
        .or_else(|| caps.get(2))
        .map(|m| m.as_str().to_string())
}

fn is_meson_assignment(line: &str) -> bool {
    let re = regex::Regex::new(r"^\w+\s*=\s*\w+\(").unwrap();
    re.is_match(line)
}

fn count_chars(s: &str, c: char) -> usize {
    s.chars().filter(|&ch| ch == c).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meson_executable() {
        let source = "project('myapp', 'cpp')\n\nmyapp_exe = executable('myapp',\n    'main.cpp',\n    'engine.cpp',\n    link_with: mylib,\n)\n";
        let chunks = MesonChunker::chunk(source, "meson.build");

        let exe = chunks.iter().find(|c| c.chunk_type == ChunkType::Module);
        assert!(
            exe.is_some(),
            "Should find an executable chunk, got: {:?}",
            chunks
                .iter()
                .map(|c| (&c.chunk_type, &c.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_meson_subdir() {
        let source = "subdir('src')\nsubdir('tests')\n";
        let chunks = MesonChunker::chunk(source, "meson.build");

        let imports: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Import)
            .collect();
        assert!(imports.len() >= 2, "Should find subdir imports");
    }

    #[test]
    fn test_meson_dependency() {
        let source = "fmt_dep = dependency('fmt', required: true)\n";
        let chunks = MesonChunker::chunk(source, "meson.build");

        let dep = chunks.iter().find(|c| c.chunk_type == ChunkType::Import);
        assert!(dep.is_some(), "Should find a dependency import");
    }
}
