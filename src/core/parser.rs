use tree_sitter::{Parser, Tree, Node};

/// Information about a code structure (function, struct, impl, etc.)
#[derive(Debug, Clone)]
pub struct StructureInfo {
    /// Type of structure: "fn", "struct", "enum", "impl", "trait", "mod"
    pub kind: String,
    /// Name of the structure (if any)
    pub name: Option<String>,
    /// 0-indexed start line
    pub start_line: usize,
    /// 0-indexed end line
    pub end_line: usize,
}

impl std::fmt::Display for StructureInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{} {} (lines {}-{})", self.kind, name, self.start_line + 1, self.end_line + 1),
            None => write!(f, "{} (lines {}-{})", self.kind, self.start_line + 1, self.end_line + 1),
        }
    }
}

/// Result of a smart read operation
#[derive(Debug)]
pub struct SmartReadResult {
    /// Adjusted end line (exclusive), may be extended to include complete structures
    pub adjusted_end: usize,
    /// The structure that caused the extension (if any)
    pub extended_structure: Option<StructureInfo>,
}

/// Parsed Rust file with AST and source code
pub struct ParsedFile {
    tree: Tree,
    #[allow(dead_code)]
    source: String,
}

impl ParsedFile {
    /// Parse Rust source code. Returns None if parsing fails.
    pub fn parse(source: String) -> Option<Self> {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE;
        parser.set_language(&language.into()).ok()?;
        let tree = parser.parse(&source, None)?;
        Some(Self { tree, source })
    }

    /// Smart read: given start and limit, return the adjusted end line
    /// that doesn't cut code structures in half.
    pub fn smart_read(&self, start: usize, limit: usize, total_lines: usize) -> SmartReadResult {
        let end = (start + limit).min(total_lines);
        let truncated = end < total_lines;

        if !truncated {
            return SmartReadResult {
                adjusted_end: end,
                extended_structure: None,
            };
        }

        if let Some(structure) = self.find_enclosing_structure(end) {
            let structure_end = structure.end_line + 1;
            if structure_end > end {
                return SmartReadResult {
                    adjusted_end: structure_end.min(total_lines),
                    extended_structure: Some(structure),
                };
            }
        }

        SmartReadResult {
            adjusted_end: end,
            extended_structure: None,
        }
    }

    /// Find the enclosing code structure at a given line (0-indexed).
    /// Returns the most specific structure containing that line.
    pub fn find_enclosing_structure(&self, line: usize) -> Option<StructureInfo> {
        let root = self.tree.root_node();
        self.find_enclosing_node(root, line)
    }

    /// Recursively find the enclosing node
    fn find_enclosing_node(&self, node: Node, line: usize) -> Option<StructureInfo> {
        let start = node.start_position().row;
        let end = node.end_position().row;

        if line < start || line > end {
            return None;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(structure) = self.find_enclosing_node(child, line) {
                return Some(structure);
            }
        }

        let kind = match node.kind() {
            "function_item" => "fn",
            "struct_item" => "struct",
            "enum_item" => "enum",
            "impl_item" => "impl",
            "trait_item" => "trait",
            "mod_item" => "mod",
            _ => return None,
        };

        let name = node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(self.source.as_bytes()).ok())
            .map(|s| s.to_string());

        Some(StructureInfo {
            kind: kind.to_string(),
            name,
            start_line: start,
            end_line: end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_function() {
        let source = r#"
fn main() {
    println!("Hello");
}

fn other() {
    println!("Other");
}
"#;
        let parsed = ParsedFile::parse(source.to_string()).unwrap();
        
        let structure = parsed.find_enclosing_structure(2).unwrap();
        assert_eq!(structure.kind, "fn");
        assert_eq!(structure.name, Some("main".to_string()));
        assert_eq!(structure.start_line, 1);
        assert_eq!(structure.end_line, 3);
    }

    #[test]
    fn test_parse_impl_block() {
        let source = r#"
struct Config {
    value: i32,
}

impl Config {
    fn new() -> Self {
        Self { value: 0 }
    }
    
    fn get(&self) -> i32 {
        self.value
    }
}
"#;
        let parsed = ParsedFile::parse(source.to_string()).unwrap();
        
        let structure = parsed.find_enclosing_structure(7).unwrap();
        assert_eq!(structure.kind, "fn");
        assert_eq!(structure.name, Some("new".to_string()));
    }

    #[test]
    fn test_line_outside_structure() {
        let source = r#"
// This is a comment

fn main() {
    println!("Hello");
}
"#;
        let parsed = ParsedFile::parse(source.to_string()).unwrap();
        
        let structure = parsed.find_enclosing_structure(1);
        assert!(structure.is_none());
    }
}
