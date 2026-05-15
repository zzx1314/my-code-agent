use my_code_agent::core::parser::{Language, ParsedFile};

// =============================================================================
// Language detection
// =============================================================================

#[test]
fn test_language_from_extension() {
    assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
    assert_eq!(Language::from_extension("js"), Some(Language::JavaScript));
    assert_eq!(Language::from_extension("jsx"), Some(Language::JavaScript));
    assert_eq!(Language::from_extension("mjs"), Some(Language::JavaScript));
    assert_eq!(Language::from_extension("cjs"), Some(Language::JavaScript));
    assert_eq!(Language::from_extension("html"), Some(Language::Html));
    assert_eq!(Language::from_extension("htm"), Some(Language::Html));
    assert_eq!(Language::from_extension("vue"), Some(Language::Vue));
    assert_eq!(Language::from_extension("py"), None);
    assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
}

#[test]
fn test_language_from_path() {
    assert_eq!(Language::from_path("src/main.rs"), Some(Language::Rust));
    assert_eq!(Language::from_path("app.js"), Some(Language::JavaScript));
    assert_eq!(Language::from_path("page.html"), Some(Language::Html));
    assert_eq!(Language::from_path("App.vue"), Some(Language::Vue));
    assert_eq!(Language::from_path("noext"), None);
    assert_eq!(Language::from_path("src/file.ts"), Some(Language::TypeScript));
}

// =============================================================================
// JavaScript parsing
// =============================================================================

#[test]
fn test_js_parse_function() {
    let source = r#"function hello() {
    return 1;
}

function world() {
    return 2;
}
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "test.js").unwrap();
    let structures = parsed.get_all_structures();

    assert_eq!(structures.len(), 2);
    assert_eq!(structures[0].kind, "function");
    assert_eq!(structures[0].name, Some("hello".to_string()));
    assert_eq!(structures[1].kind, "function");
    assert_eq!(structures[1].name, Some("world".to_string()));
}

#[test]
fn test_js_parse_class() {
    let source = r#"class Animal {
    constructor(name) {
        this.name = name;
    }

    speak() {
        return this.name + ' speaks';
    }
}

function standalone() {}
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "test.js").unwrap();
    let structures = parsed.get_all_structures();

    // Should have: Animal class, constructor method, speak method, standalone function
    assert!(
        structures.len() >= 3,
        "Expected at least 3 structures, got {}",
        structures.len()
    );

    // Find class
    let class = structures
        .iter()
        .find(|s| s.kind == "class" && s.name == Some("Animal".to_string()));
    assert!(class.is_some(), "Should find Animal class");

    // Find standalone function
    let standalone = structures
        .iter()
        .find(|s| s.kind == "function" && s.name == Some("standalone".to_string()));
    assert!(standalone.is_some(), "Should find standalone function");
}

#[test]
fn test_js_parse_const_arrow_function() {
    let source = r#"const greet = () => {
    return 'hello';
};

const add = (a, b) => a + b;

const notAFunction = 42;
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "test.js").unwrap();
    let structures = parsed.get_all_structures();

    // Should pick up const with arrow functions
    let greet = structures
        .iter()
        .find(|s| s.name == Some("greet".to_string()));
    assert!(greet.is_some(), "Should find greet const");

    let add = structures
        .iter()
        .find(|s| s.name == Some("add".to_string()));
    assert!(add.is_some(), "Should find add const");

    // notAFunction should NOT appear (it's a const, not a function)
    let not_fn = structures
        .iter()
        .find(|s| s.name == Some("notAFunction".to_string()));
    assert!(
        not_fn.is_none(),
        "Should not find notAFunction as a structure"
    );
}

#[test]
fn test_js_find_enclosing_structure() {
    let source = r#"function hello() {
    console.log('world');
}

class Foo {
    bar() {}
}
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "test.js").unwrap();

    // Line 1 (0-indexed) is inside hello()
    let structure = parsed.find_enclosing_structure(1).unwrap();
    assert_eq!(structure.kind, "function");
    assert_eq!(structure.name, Some("hello".to_string()));

    // Line 0 is hello() definition line
    let structure = parsed.find_enclosing_structure(0).unwrap();
    assert_eq!(structure.kind, "function");

    // Line 5 is inside Foo's bar method
    let structure = parsed.find_enclosing_structure(5).unwrap();
    assert_eq!(structure.kind, "method");
    assert_eq!(structure.name, Some("bar".to_string()));
}

#[test]
fn test_js_parse_none_jsx_file() {
    // Ensure .py returns None
    let parsed = ParsedFile::parse_with_path("x = 1".to_string(), "test.py");
    assert!(parsed.is_none());
}

// =============================================================================
// HTML parsing
// =============================================================================

#[test]
fn test_html_parse_elements() {
    let source = r#"<html>
<body>
    <div id="app">Hello</div>
    <script>var x = 1;</script>
</body>
</html>
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "test.html").unwrap();
    let structures = parsed.get_all_structures();

    assert!(!structures.is_empty(), "Should find some HTML elements");
    // Should find at least an element
    assert!(
        structures.iter().any(|s| s.kind == "element"),
        "Should find elements"
    );
}

#[test]
fn test_html_smart_read() {
    let source = r#"line1
line2
<html>
<body>
<div>
<p>content</p>
</div>
</body>
</html>
more
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "test.html").unwrap();
    let total_lines = source.lines().count();

    // Smart read should not crash
    let result = parsed.smart_read(0, 5, total_lines);
    assert!(result.adjusted_end > 0);
}

// =============================================================================
// Vue SFC parsing
// =============================================================================

#[test]
fn test_vue_parse_sfc_blocks() {
    let source = r#"<template>
  <div class="app">
    <h1>Hello</h1>
    <p>{{ message }}</p>
  </div>
</template>

<script>
export default {
  data() {
    return { message: 'Hello' };
  },
  methods: {
    greet() {
      alert(this.message);
    }
  }
};
</script>

<style scoped>
.app { color: red; }
</style>
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "App.vue").unwrap();
    let structures = parsed.get_all_structures();

    // Should find template, script, style blocks
    let template = structures.iter().find(|s| s.kind == "template");
    assert!(template.is_some(), "Should find template block");

    let script = structures.iter().find(|s| s.kind == "script");
    assert!(script.is_some(), "Should find script block");

    let style = structures.iter().find(|s| s.kind == "style");
    assert!(style.is_some(), "Should find style block");
}

#[test]
fn test_vue_parse_script_js_structures() {
    let source = r#"<template>
  <div>{{ msg }}</div>
</template>

<script>
function helper() {
  return 42;
}

class ApiClient {
  constructor(baseUrl) {
    this.baseUrl = baseUrl;
  }

  fetch() {
    return fetch(this.baseUrl);
  }
}
</script>
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "App.vue").unwrap();
    let structures = parsed.get_all_structures();

    // Should find JS structures inside <script> with correct line numbers
    let helper = structures
        .iter()
        .find(|s| s.name == Some("helper".to_string()));
    assert!(helper.is_some(), "Should find helper() inside script block");
    assert_eq!(helper.unwrap().kind, "function");

    let api = structures
        .iter()
        .find(|s| s.name == Some("ApiClient".to_string()));
    assert!(
        api.is_some(),
        "Should find ApiClient class inside script block"
    );
    assert_eq!(api.unwrap().kind, "class");
}

#[test]
fn test_vue_find_enclosing_structure() {
    let source = r#"<template>
  <div>{{ msg }}</div>
</template>

<script>
function helper() {
  return 42;
}
</script>

<style>
.app { color: red; }
</style>
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "App.vue").unwrap();

    // Line 0 is inside <template>
    let structure = parsed.find_enclosing_structure(0);
    assert!(structure.is_some(), "Line 0 should be inside template");
    assert_eq!(structure.unwrap().kind, "template");

    // Line 5 is inside <script>'s function helper
    let structure = parsed.find_enclosing_structure(5);
    assert!(structure.is_some(), "Line 5 should be inside a structure");
    let s = structure.unwrap();
    // Could be either script or function, depending on nesting depth
    assert!(
        s.kind == "function" || s.kind == "script",
        "Line 5 should be inside function or script, got: {} {}",
        s.kind,
        s.name.as_deref().unwrap_or("none")
    );
}

#[test]
fn test_vue_parse_empty_script() {
    let source = r#"<template>
  <div>Hello</div>
</template>

<script>
</script>
"#;
    let parsed = ParsedFile::parse_with_path(source.to_string(), "App.vue").unwrap();
    let structures = parsed.get_all_structures();

    let template = structures.iter().find(|s| s.kind == "template");
    assert!(template.is_some());

    let script = structures.iter().find(|s| s.kind == "script");
    assert!(script.is_some());

    // No JS structures in empty script
    let js_structures: Vec<_> = structures
        .iter()
        .filter(|s| s.kind == "function" || s.kind == "class")
        .collect();
    assert!(
        js_structures.is_empty(),
        "Empty script should not produce JS structures"
    );
}

// =============================================================================
// Rust backward compatibility (ensure legacy parse still works)
// =============================================================================

#[test]
fn test_rust_parse_still_works() {
    let source = r#"
fn main() {
    println!("Hello");
}

struct Config {
    value: i32,
}
"#;
    let parsed = ParsedFile::parse(source.to_string()).unwrap();
    let structures = parsed.get_all_structures();

    assert!(structures.len() >= 2);
    let main_fn = structures
        .iter()
        .find(|s| s.name == Some("main".to_string()));
    assert!(main_fn.is_some());
    let config = structures
        .iter()
        .find(|s| s.name == Some("Config".to_string()));
    assert!(config.is_some());
}

#[test]
fn test_vue_outline_display_demo() {
    let source = "<template>
  <div class=\"app\">
    <h1>{{ title }}</h1>
    <button @click=\"handleClick\">Click me</button>
    <ChildComponent :data=\"items\" />
  </div>
</template>

<script>
import ChildComponent from './ChildComponent.vue'

const MAX_ITEMS = 100

export default {
  name: 'MyApp',
  components: { ChildComponent },
  data() {
    return {
      title: 'Hello Vue',
      items: []
    }
  },
  methods: {
    handleClick() {
      console.log('clicked')
    },
    async fetchItems() {
      const res = await fetch('/api/items')
      this.items = await res.json()
    }
  },
  computed: {
    itemCount() {
      return this.items.length
    }
  }
}
</script>

<style scoped>
.app {
  max-width: 800px;
  margin: 0 auto;
}
</style>
";
    let parsed = ParsedFile::parse_with_path(source.to_string(), "App.vue").unwrap();
    let structures = parsed.get_all_structures();

    println!(
        "\n=== Vue SFC Outline ({} structures) ===",
        structures.len()
    );
    for s in &structures {
        let display_name = s.name.as_deref().unwrap_or("(unnamed)");
        println!(
            "  {:12} {:20} lines {:3}-{:3}",
            s.kind,
            display_name,
            s.start_line + 1,
            s.end_line + 1
        );
    }
    println!("=============================\n");

    assert!(structures.len() >= 3, "Should detect Vue SFC blocks");
}
