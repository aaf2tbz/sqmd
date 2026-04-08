use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::Tree;

pub struct CMakeChunker;

impl Default for CMakeChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl CMakeChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for CMakeChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_cmake::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "cmake"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            let kind = child.kind();

            match kind {
                "function_def" | "macro_def" => {
                    let name = extract_cmake_def_name(&child, source);
                    let sig = child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    let ct = if kind == "macro_def" {
                        ChunkType::Macro
                    } else {
                        ChunkType::Function
                    };

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cmake",
                        ct,
                        name.as_deref(),
                        sig.as_deref(),
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "normal_command" => {
                    let cmd_name = extract_cmake_command_name(&child, source);
                    let cmd_lower = cmd_name.as_deref().unwrap_or("").to_lowercase();
                    let cmd_lower_str = cmd_lower.as_str();

                    match cmd_lower_str {
                        "add_library" | "add_executable" => {
                            if let Some(chunk) = make_chunk(
                                source,
                                child,
                                file_path,
                                "cmake",
                                ChunkType::Module,
                                cmd_name.as_deref(),
                                None,
                                serde_json::Map::new(),
                            ) {
                                chunks.push(chunk);
                            }
                        }
                        "find_package"
                        | "find_library"
                        | "find_path"
                        | "find_program"
                        | "fetchcontent_declare"
                        | "cpmaddpackage" => {
                            if let Some(chunk) = make_chunk(
                                source,
                                child,
                                file_path,
                                "cmake",
                                ChunkType::Import,
                                cmd_name.as_deref(),
                                None,
                                serde_json::Map::new(),
                            ) {
                                chunks.push(chunk);
                            }
                        }
                        "set" | "option" => {
                            let text = child.utf8_text(source.as_bytes()).ok().unwrap_or("");
                            if text.lines().count() <= 3 {
                                if let Some(chunk) = make_chunk(
                                    source,
                                    child,
                                    file_path,
                                    "cmake",
                                    ChunkType::Constant,
                                    None,
                                    None,
                                    serde_json::Map::new(),
                                ) {
                                    chunks.push(chunk);
                                }
                            }
                        }
                        "add_subdirectory" => {
                            if let Some(chunk) = make_chunk(
                                source,
                                child,
                                file_path,
                                "cmake",
                                ChunkType::Import,
                                None,
                                None,
                                serde_json::Map::new(),
                            ) {
                                chunks.push(chunk);
                            }
                        }
                        "target_link_libraries"
                        | "target_include_directories"
                        | "target_compile_definitions"
                        | "target_compile_options" => {
                            if let Some(chunk) = make_chunk(
                                source,
                                child,
                                file_path,
                                "cmake",
                                ChunkType::Export,
                                None,
                                None,
                                serde_json::Map::new(),
                            ) {
                                chunks.push(chunk);
                            }
                        }
                        _ => {}
                    }
                }
                "if_condition" | "foreach_loop" | "while_loop" => {
                    let sig = child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cmake",
                        ChunkType::Section,
                        None,
                        sig.as_deref(),
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let mut imports = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() != "normal_command" {
                continue;
            }
            let cmd_name = extract_cmake_command_name(&child, source);
            let cmd_lower = cmd_name.as_deref().unwrap_or("").to_lowercase();

            match cmd_lower.as_str() {
                "find_package"
                | "add_subdirectory"
                | "include"
                | "fetchcontent_declare"
                | "cpmaddpackage" => {
                    if let Some(arg) = extract_first_argument(&child, source) {
                        if seen.insert(arg.clone()) {
                            imports.push(crate::relationships::ImportInfo {
                                source_file: String::new(),
                                module_path: arg,
                                names: Vec::new(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        imports
    }

    fn chunk_unclaimed(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        crate::chunker::FileChunker::chunk_file_into(source, file_path, "cmake", chunks);
    }
}

fn extract_cmake_command_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            if let Ok(name) = child.utf8_text(source.as_bytes()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn extract_cmake_def_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_command" || child.kind() == "macro_command" {
            return extract_cmake_def_first_arg(&child, source);
        }
    }
    None
}

fn extract_cmake_def_first_arg(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "argument_list" {
            for arg_child in child.children(&mut child.walk()) {
                if arg_child.kind() == "argument" {
                    if let Ok(text) = arg_child.utf8_text(source.as_bytes()) {
                        let arg = text.trim().to_string();
                        if !arg.is_empty() {
                            return Some(arg);
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_first_argument(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "argument_list" {
            for arg_child in child.children(&mut child.walk()) {
                if arg_child.kind() == "argument" {
                    if let Ok(text) = arg_child.utf8_text(source.as_bytes()) {
                        let arg = text.trim().to_string();
                        if !arg.is_empty() {
                            return Some(arg);
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmake_function() {
        let source = "cmake_minimum_required(VERSION 3.20)\n\nfunction(add_plugin NAME)\n    add_library(${NAME} STATIC ${ARGN})\nendfunction()\n";
        let chunker = CMakeChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "plugins.cmake");

        let func = chunks.iter().find(|c| c.chunk_type == ChunkType::Function);
        assert!(
            func.is_some(),
            "Should find a function chunk, got: {:?}",
            chunks
                .iter()
                .map(|c| (&c.chunk_type, &c.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_cmake_imports() {
        let source = "find_package(Qt6 REQUIRED COMPONENTS Core Quick)\nadd_subdirectory(src)\nfind_package(fmt REQUIRED)\n";
        let chunker = CMakeChunker::new();
        let (_chunks, tree) = chunker.chunk(source, "CMakeLists.txt");
        let imports = chunker.extract_imports(&tree.unwrap(), source);
        assert!(
            imports.len() >= 2,
            "Should find at least 2 imports, got {}",
            imports.len()
        );
        assert!(imports.iter().any(|i| i.module_path.contains("Qt6")));
        assert!(imports.iter().any(|i| i.module_path == "src"));
    }

    #[test]
    fn test_cmake_targets() {
        let source = "add_library(zrythm_core STATIC engine.cpp mixer.cpp)\nadd_executable(zrythm main.cpp)\n";
        let chunker = CMakeChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "CMakeLists.txt");

        let targets: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Module)
            .collect();
        assert!(targets.len() >= 2, "Should find target chunks");
    }
}
