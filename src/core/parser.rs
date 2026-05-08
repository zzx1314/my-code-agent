use tree_sitter::{Node, Parser, Tree};

/// Supported programming languages for tree-sitter parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    JavaScript,
    Html,
    Vue,
}

impl Language {
    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Language::Rust),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "html" | "htm" => Some(Language::Html),
            "vue" => Some(Language::Vue),
            _ => None,
        }
    }

    /// Detect language from file path
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?;
        Self::from_extension(ext)
    }
}

/// Information about a code structure (function, struct, impl, etc.)
#[derive(Debug, Clone)]
pub struct StructureInfo {
    /// Type of structure: "fn", "struct", "enum", "impl", "trait", "mod", "function", "class", "method", "const", "template", "script", "style"
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
            Some(name) => write!(
                f,
                "{} {} (lines {}-{})",
                self.kind,
                name,
                self.start_line + 1,
                self.end_line + 1
            ),
            None => write!(
                f,
                "{} (lines {}-{})",
                self.kind,
                self.start_line + 1,
                self.end_line + 1
            ),
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

/// Parsed source file with AST and source code
pub struct ParsedFile {
    tree: Tree,
    #[allow(dead_code)]
    source: String,
    language: Language,
}

impl ParsedFile {
    /// Parse source code with auto-detected language from file path.
    /// Returns None if parsing fails or language is not supported.
    pub fn parse_with_path(source: String, path: &str) -> Option<Self> {
        let language = Language::from_path(path)?;
        Self::parse_with_language(source, language)
    }

    /// Parse source code for a specific language.
    pub fn parse_with_language(source: String, language: Language) -> Option<Self> {
        let lang_fn = match language {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Language::Html | Language::Vue => tree_sitter_html::LANGUAGE.into(),
        };
        let mut parser = Parser::new();
        parser.set_language(&lang_fn).ok()?;
        let tree = parser.parse(&source, None)?;
        Some(Self {
            tree,
            source,
            language,
        })
    }

    /// Parse source code (legacy API - assumes Rust for backward compatibility).
    pub fn parse(source: String) -> Option<Self> {
        Self::parse_with_language(source, Language::Rust)
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

        let kind = match self.language {
            Language::Rust => Self::rust_node_kind(node),
            Language::JavaScript => Self::js_node_kind(node, &self.source),
            Language::Html => Self::html_node_kind(node),
            Language::Vue => Self::vue_node_kind(node, &self.source),
        };

        let kind = kind?;

        let name = self.get_node_name(node);

        Some(StructureInfo {
            kind: kind.to_string(),
            name,
            start_line: start,
            end_line: end,
        })
    }

    /// Get all top-level structures in the file
    pub fn get_all_structures(&self) -> Vec<StructureInfo> {
        let root = self.tree.root_node();
        match self.language {
            Language::Vue => self.collect_vue_structures(root),
            _ => self.collect_structures(root, true),
        }
    }

    /// Get the language of this parsed file
    pub fn language(&self) -> Language {
        self.language
    }

    /// Map Rust AST node kinds to structure types
    fn rust_node_kind(node: Node) -> Option<&'static str> {
        match node.kind() {
            "function_item" => Some("fn"),
            "struct_item" => Some("struct"),
            "enum_item" => Some("enum"),
            "impl_item" => Some("impl"),
            "trait_item" => Some("trait"),
            "mod_item" => Some("mod"),
            _ => None,
        }
    }

    /// Map JavaScript AST node kinds to structure types
    fn js_node_kind(node: Node, source: &str) -> Option<&'static str> {
        match node.kind() {
            "function_declaration" => Some("function"),
            "class_declaration" => Some("class"),
            "method_definition" => Some("method"),
            "lexical_declaration" => {
                // Only treat const/let with arrow functions or function expressions as structures
                if Self::has_function_value(node, source) {
                    Some("const")
                } else {
                    None
                }
            }
            "variable_declaration" => {
                // var with function value
                if Self::has_function_value(node, source) {
                    Some("var")
                } else {
                    None
                }
            }
            "export_statement" => {
                // export default / export function / export class
                // Check if the child is a declaration we care about
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(kind) = Self::js_node_kind(child, source) {
                        return Some(kind);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Check if a variable/lexical declaration has a function value
    fn has_function_value(node: Node, _source: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "variable_declarator" => {
                    let mut c2 = child.walk();
                    for sub in child.children(&mut c2) {
                        if matches!(sub.kind(), "arrow_function" | "function") {
                            return true;
                        }
                    }
                }
                _ => continue,
            }
        }
        false
    }

    /// Map HTML AST node kinds to structure types
    fn html_node_kind(node: Node) -> Option<&'static str> {
        match node.kind() {
            "element" | "script_element" | "style_element" => {
                // Get the tag name for more descriptive output
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "start_tag" {
                        let mut c2 = child.walk();
                        for sub in child.children(&mut c2) {
                            if sub.kind() == "tag_name" {
                                // We'll use a generic "element" kind; name carries the tag
                            }
                        }
                    }
                }
                Some("element")
            }
            _ => None,
        }
    }

    /// Map Vue SFC node kinds to structure types.
    /// Vue files are parsed as HTML, so we detect <template>, <script>, <style> blocks.
    fn vue_node_kind(node: Node, source: &str) -> Option<&'static str> {
        match node.kind() {
            "script_element" => Some("script"),
            "style_element" => Some("style"),
            "element" => {
                // Check if it's a <template> top-level element
                let tag = Self::get_element_tag(node, source);
                match tag.as_deref() {
                    Some("template") => Some("template"),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Get the tag name of an HTML element node
    fn get_element_tag(node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "start_tag" {
                let mut c2 = child.walk();
                for sub in child.children(&mut c2) {
                    if sub.kind() == "tag_name" {
                        return sub
                            .utf8_text(source.as_bytes())
                            .ok()
                            .map(|s| s.to_string());
                    }
                }
            }
        }
        None
    }

    /// Get the name for a node based on the current language
    fn get_node_name(&self, node: Node) -> Option<String> {
        match self.language {
            Language::JavaScript => {
                // For JS nodes, try "name" field first
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(text) = name_node.utf8_text(self.source.as_bytes()) {
                        return Some(text.to_string());
                    }
                }
                // For lexical_declaration (const/let) and variable_declaration (var),
                // look into variable_declarator children for the name
                if matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "variable_declarator" {
                            if let Some(name_node) = child.child_by_field_name("name") {
                                if let Ok(text) = name_node.utf8_text(self.source.as_bytes()) {
                                    return Some(text.to_string());
                                }
                            }
                        }
                    }
                }
                // For export_statement, look for a declaration inside
                if node.kind() == "export_statement" {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if let Some(name) = self.get_node_name(child) {
                            return Some(name);
                        }
                    }
                }
                None
            }
            Language::Vue => {
                // For Vue SFC, elements get their tag name as the name
                if let Some(tag) = Self::get_element_tag(node, &self.source) {
                    return Some(tag);
                }
                // For script_element and style_element, get tag name from start_tag
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "start_tag" {
                        let mut c2 = child.walk();
                        for sub in child.children(&mut c2) {
                            if sub.kind() == "tag_name" {
                                if let Ok(text) = sub.utf8_text(self.source.as_bytes()) {
                                    return Some(text.to_string());
                                }
                            }
                        }
                    }
                }
                None
            }
            Language::Rust | Language::Html => {
                node.child_by_field_name("name")
                    .and_then(|n| n.utf8_text(self.source.as_bytes()).ok())
                    .map(|s| s.to_string())
            }
        }
    }

    /// Recursively collect structures from a node
    fn collect_structures(&self, node: Node, _top_level_only: bool) -> Vec<StructureInfo> {
        let mut structures = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            let kind = match self.language {
                Language::Rust => Self::rust_node_kind(child),
                Language::JavaScript => Self::js_node_kind(child, &self.source),
                Language::Html => Self::html_node_kind(child),
                Language::Vue => Self::vue_node_kind(child, &self.source),
            };

            let kind = match kind {
                Some(k) => k,
                None => {
                    // Always recurse into non-structure nodes to find nested structures
                    structures.extend(self.collect_structures(child, false));
                    continue;
                }
            };

            let name = self.get_node_name(child);

            structures.push(StructureInfo {
                kind: kind.to_string(),
                name,
                start_line: child.start_position().row,
                end_line: child.end_position().row,
            });

            // For impl blocks (Rust), also collect methods inside
            if self.language == Language::Rust && kind == "impl" {
                structures.extend(self.collect_structures(child, false));
            }
            // For class declarations (JS), also collect methods inside
            if self.language == Language::JavaScript && kind == "class" {
                structures.extend(self.collect_structures(child, false));
            }
        }

        structures
    }

    /// Collect Vue SFC structures and also parse <script> content for JS structures
    fn collect_vue_structures(&self, root: Node) -> Vec<StructureInfo> {
        let mut structures = Vec::new();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            let kind = Self::vue_node_kind(child, &self.source);

            if let Some(kind_str) = kind {
                let name = self.get_node_name(child);
                structures.push(StructureInfo {
                    kind: kind_str.to_string(),
                    name,
                    start_line: child.start_position().row,
                    end_line: child.end_position().row,
                });

                // For <script> blocks, also parse the JS content inside
                if kind_str == "script" {
                    if let Some(js_structures) = self.parse_vue_script(child) {
                        structures.extend(js_structures);
                    }
                }
            }
        }

        structures
    }

    /// Parse JavaScript inside a Vue <script> element and return JS structures
    /// with line numbers adjusted to the file's coordinate system.
    fn parse_vue_script(&self, script_node: Node) -> Option<Vec<StructureInfo>> {
        // Find the raw_text child of the script_element
        let mut cursor = script_node.walk();
        let script_offset = script_node.start_position().row;
        let mut raw_text_node: Option<Node> = None;

        for child in script_node.children(&mut cursor) {
            if child.kind() == "raw_text" {
                raw_text_node = Some(child);
                break;
            }
        }

        let raw_text = raw_text_node?;
        let js_source = raw_text.utf8_text(self.source.as_bytes()).ok()?;

        // Parse the JS content
        let mut parser = Parser::new();
        let lang_fn = tree_sitter_javascript::LANGUAGE;
        parser.set_language(&lang_fn.into()).ok()?;
        let js_tree = parser.parse(js_source, None)?;
        let js_root = js_tree.root_node();

        // Collect JS structures with adjusted line numbers
        let mut structures = Vec::new();
        let mut js_cursor = js_root.walk();

        for child in js_root.children(&mut js_cursor) {
            let kind = Self::js_node_kind(child, js_source);
            if let Some(kind_str) = kind {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(js_source.as_bytes()).ok())
                    .map(|s| s.to_string());

                structures.push(StructureInfo {
                    kind: kind_str.to_string(),
                    name,
                    start_line: child.start_position().row + script_offset,
                    end_line: child.end_position().row + script_offset,
                });

                // Collect methods inside classes
                if kind_str == "class" {
                    let mut class_cursor = child.walk();
                    for class_child in child.children(&mut class_cursor) {
                        let sub_kind = Self::js_node_kind(class_child, js_source);
                        if let Some(sub_kind_str) = sub_kind {
                            let sub_name = class_child
                                .child_by_field_name("name")
                                .and_then(|n| n.utf8_text(js_source.as_bytes()).ok())
                                .map(|s| s.to_string());
                            structures.push(StructureInfo {
                                kind: sub_kind_str.to_string(),
                                name: sub_name,
                                start_line: class_child.start_position().row + script_offset,
                                end_line: class_child.end_position().row + script_offset,
                            });
                        }
                    }
                }
            }
        }

        Some(structures)
    }
}
