use tree_sitter::{Node, Parser, Tree};

/// Supported programming languages for tree-sitter parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    JavaScript,
    TypeScript,
    Java,
    Html,
    Vue,
}

impl Language {
    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Language::Rust),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "ts" | "tsx" | "mts" | "cts" => Some(Language::TypeScript),
            "java" => Some(Language::Java),
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
    /// Type of structure: "fn", "struct", "enum", "impl", "trait", "mod",
    /// "function", "class", "method", "const", "template", "script", "style",
    /// "vue:methods", "vue:computed", "vue:watch"
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
                "{} (lines {}-{})",
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
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
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
            Language::TypeScript => Self::ts_node_kind(node, &self.source),
            Language::Java => Self::java_node_kind(node),
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

    /// Map TypeScript AST node kinds to structure types.
    /// TypeScript shares the same grammar structure as JavaScript.
    fn ts_node_kind(node: Node, source: &str) -> Option<&'static str> {
        // Reuse JS logic for common JS syntax
        if let Some(kind) = Self::js_node_kind(node, source) {
            return Some(kind);
        }
        // TypeScript-specific declarations
        match node.kind() {
            "interface_declaration" => Some("interface"),
            "type_alias_declaration" => Some("type"),
            "enum_declaration" => Some("enum"),
            "abstract_class_declaration" => Some("class"),
            _ => None,
        }
    }

    /// Map Java AST node kinds to structure types
    fn java_node_kind(node: Node) -> Option<&'static str> {
        match node.kind() {
            "class_declaration" => Some("class"),
            "interface_declaration" => Some("interface"),
            "enum_declaration" => Some("enum"),
            "method_declaration" => Some("method"),
            "constructor_declaration" => Some("constructor"),
            "annotation_type_declaration" => Some("annotation"),
            "record_declaration" => Some("record"),
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
            "element" | "script_element" | "style_element" => Some("element"),
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
                        return sub.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
                    }
                }
            }
        }
        None
    }

    /// Get the name for a node based on the current language
    fn get_node_name(&self, node: Node) -> Option<String> {
        match self.language {
            Language::JavaScript | Language::TypeScript => Self::get_js_node_name(node, &self.source),
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
            Language::Rust | Language::Html | Language::Java => node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(self.source.as_bytes()).ok())
                .map(|s| s.to_string()),
        }
    }

    /// Get name for a JS node (without needing &self - uses js_source directly)
    fn get_js_node_name(node: Node, js_source: &str) -> Option<String> {
        // Try "name" field first
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(text) = name_node.utf8_text(js_source.as_bytes()) {
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
                        if let Ok(text) = name_node.utf8_text(js_source.as_bytes()) {
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
                if let Some(name) = Self::get_js_node_name(child, js_source) {
                    return Some(name);
                }
            }
        }
        None
    }

    /// Recursively collect structures from a node
    fn collect_structures(&self, node: Node, _top_level_only: bool) -> Vec<StructureInfo> {
        let mut structures = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            let kind = match self.language {
                Language::Rust => Self::rust_node_kind(child),
                Language::JavaScript => Self::js_node_kind(child, &self.source),
                Language::TypeScript => Self::ts_node_kind(child, &self.source),
                Language::Java => Self::java_node_kind(child),
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
            // For class declarations (JS/TS), also collect methods inside
            if matches!(self.language, Language::JavaScript | Language::TypeScript) && kind == "class" {
                structures.extend(self.collect_structures(child, false));
            }
            // For class declarations (Java), also collect methods and constructors inside
            if self.language == Language::Java && matches!(kind, "class" | "interface" | "enum" | "record") {
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
    /// Supports Vue Options API (methods, computed, data, etc.) and Composition API.
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
            self.collect_js_structures(child, js_source, script_offset, &mut structures);
        }

        Some(structures)
    }

    /// Recursively collect JavaScript structures from a JS AST node.
    /// Handles functions, classes, methods, const/let/var with function values,
    /// and Vue Options API patterns (methods, computed, data, watch, lifecycle hooks).
    fn collect_js_structures(
        &self,
        node: Node,
        js_source: &str,
        offset: usize,
        structures: &mut Vec<StructureInfo>,
    ) {
        let kind = Self::js_node_kind(node, js_source);

        if let Some(kind_str) = kind {
            let name = Self::get_js_node_name(node, js_source);

            structures.push(StructureInfo {
                kind: kind_str.to_string(),
                name,
                start_line: node.start_position().row + offset,
                end_line: node.end_position().row + offset,
            });

            // For classes, recurse to collect methods inside
            if kind_str == "class" {
                let mut class_cursor = node.walk();
                for class_child in node.children(&mut class_cursor) {
                    self.collect_js_structures(class_child, js_source, offset, structures);
                }
            }
            // For export_statement, recurse into the inner declaration
            if node.kind() == "export_statement" {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_js_structures(child, js_source, offset, structures);
                }
            }
        } else {
            // Not a recognized JS structure at this level.
            // Check for Vue Options API patterns:
            // - export default { methods: {...}, computed: {...}, data() {...}, ... }
            // - object properties that contain method definitions
            self.collect_vue_options_structures(node, js_source, offset, structures);
        }
    }

    /// Collect structures from Vue Options API patterns.
    /// Looks inside `export default { ... }` for methods, computed, data, watch,
    /// lifecycle hooks (mounted, created, etc.), and other option properties.
    fn collect_vue_options_structures(
        &self,
        node: Node,
        js_source: &str,
        offset: usize,
        structures: &mut Vec<StructureInfo>,
    ) {
        let node_kind = node.kind();

        match node_kind {
            // export default { ... } -> recurse into the object expression
            "export_statement" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_js_structures(child, js_source, offset, structures);
                }
            }
            // A pair like `methods: { handleClick() {...} }` inside an object
            "pair" => {
                let key = node.child_by_field_name("key");
                let value = node.child_by_field_name("value");

                if let (Some(key_node), Some(val_node)) = (key, value) {
                    let key_text = key_node.utf8_text(js_source.as_bytes()).unwrap_or("");

                    let vue_option_keys = ["methods", "computed", "watch"];
                    let vue_lifecycle_hooks = [
                        "data",
                        "setup",
                        "beforeCreate",
                        "created",
                        "beforeMount",
                        "mounted",
                        "beforeUpdate",
                        "updated",
                        "beforeDestroy",
                        "destroyed",
                        "beforeUnmount",
                        "unmounted",
                        "activated",
                        "deactivated",
                        "errorCaptured",
                        "render",
                        "renderTracked",
                        "renderTriggered",
                    ];

                    if vue_option_keys.contains(&key_text) && val_node.kind() == "object" {
                        // This is a Vue methods/computed/watch block
                        // Emit it as a named structure group
                        structures.push(StructureInfo {
                            kind: format!("vue:{}", key_text),
                            name: Some(key_text.to_string()),
                            start_line: node.start_position().row + offset,
                            end_line: node.end_position().row + offset,
                        });

                        // Collect each method inside
                        let mut obj_cursor = val_node.walk();
                        for obj_child in val_node.children(&mut obj_cursor) {
                            self.collect_vue_object_methods(
                                obj_child, js_source, offset, structures,
                            );
                        }
                    } else if vue_lifecycle_hooks.contains(&key_text) {
                        // Lifecycle hooks and data/setup as standalone methods
                        structures.push(StructureInfo {
                            kind: "method".to_string(),
                            name: Some(key_text.to_string()),
                            start_line: node.start_position().row + offset,
                            end_line: node.end_position().row + offset,
                        });
                    } else if val_node.kind() == "object" {
                        // Other object properties: recurse to find nested functions
                        let mut obj_cursor = val_node.walk();
                        for obj_child in val_node.children(&mut obj_cursor) {
                            self.collect_vue_options_structures(
                                obj_child, js_source, offset, structures,
                            );
                        }
                    }
                }
            }
            // Shorthand method like `data() { ... }` inside an object (without pair)
            "method_definition" => {
                let name = node
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(js_source.as_bytes()).ok())
                    .map(|s| s.to_string());

                structures.push(StructureInfo {
                    kind: "method".to_string(),
                    name,
                    start_line: node.start_position().row + offset,
                    end_line: node.end_position().row + offset,
                });
            }
            // Recurse into objects and other containers
            "object" | "object_pattern" | "array" | "expression_statement" | "statement_block" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_vue_options_structures(child, js_source, offset, structures);
                }
            }
            _ => {}
        }
    }

    /// Collect methods inside a Vue options object (e.g., methods: { ... })
    fn collect_vue_object_methods(
        &self,
        node: Node,
        js_source: &str,
        offset: usize,
        structures: &mut Vec<StructureInfo>,
    ) {
        match node.kind() {
            // method: { handleClick() {...} } -> shorthand method
            "method_definition" => {
                let name = node
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(js_source.as_bytes()).ok())
                    .map(|s| s.to_string());
                structures.push(StructureInfo {
                    kind: "method".to_string(),
                    name,
                    start_line: node.start_position().row + offset,
                    end_line: node.end_position().row + offset,
                });
            }
            // handleClick: function() {...} -> pair with function value
            "pair" => {
                if let Some(name_node) = node.child_by_field_name("key") {
                    let name = name_node
                        .utf8_text(js_source.as_bytes())
                        .ok()
                        .map(|s| s.to_string());
                    structures.push(StructureInfo {
                        kind: "method".to_string(),
                        name,
                        start_line: node.start_position().row + offset,
                        end_line: node.end_position().row + offset,
                    });
                }
            }
            "spread_element" | "comment" | "," => {}
            _ => {
                // For other nodes (e.g., properties), try to get name
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = name_node
                        .utf8_text(js_source.as_bytes())
                        .ok()
                        .map(|s| s.to_string());
                    if name.is_some() {
                        structures.push(StructureInfo {
                            kind: "method".to_string(),
                            name,
                            start_line: node.start_position().row + offset,
                            end_line: node.end_position().row + offset,
                        });
                    }
                }
            }
        }
    }
}
