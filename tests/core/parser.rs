use my_code_agent::core::parser::{Language, ParsedFile};

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

#[test]
fn test_parse_typescript_function() {
    let source = r#"
function greet(name: string): string {
    return `Hello, ${name}!`;
}

interface User {
    name: string;
    age: number;
}

class Person {
    name: string;

    constructor(name: string) {
        this.name = name;
    }

    sayHello(): void {
        console.log(`Hi, I'm ${this.name}`);
    }
}
"#;
    let parsed = ParsedFile::parse_with_language(source.to_string(), Language::TypeScript).unwrap();

    // Test function detection
    let fn_structure = parsed.find_enclosing_structure(2).unwrap();
    assert_eq!(fn_structure.kind, "function");
    assert_eq!(fn_structure.name, Some("greet".to_string()));

    // Test interface detection
    let iface_structure = parsed.find_enclosing_structure(5).unwrap();
    assert_eq!(iface_structure.kind, "interface");
    assert_eq!(iface_structure.name, Some("User".to_string()));

    // Test class detection
    let class_structure = parsed.find_enclosing_structure(10).unwrap();
    assert_eq!(class_structure.kind, "class");
    assert_eq!(class_structure.name, Some("Person".to_string()));

    // Test method inside class
    let method_structure = parsed.find_enclosing_structure(17).unwrap();
    assert_eq!(method_structure.kind, "method");
    assert_eq!(method_structure.name, Some("sayHello".to_string()));

    // Test all top-level structures
    let all = parsed.get_all_structures();
    let kinds: Vec<&str> = all.iter().map(|s| s.kind.as_str()).collect();
    assert!(kinds.contains(&"function"));
    assert!(kinds.contains(&"interface"));
    assert!(kinds.contains(&"class"));
}

#[test]
fn test_parse_java_class() {
    let source = r#"
public class Calculator {
    private int result;

    public Calculator() {
        this.result = 0;
    }

    public int add(int a, int b) {
        return a + b;
    }

    public int subtract(int a, int b) {
        return a - b;
    }
}
"#;
    let parsed = ParsedFile::parse_with_language(source.to_string(), Language::Java).unwrap();

    // Test class detection
    let class_structure = parsed.find_enclosing_structure(2).unwrap();
    assert_eq!(class_structure.kind, "class");
    assert_eq!(class_structure.name, Some("Calculator".to_string()));

    // Test constructor detection
    let ctor_structure = parsed.find_enclosing_structure(5).unwrap();
    assert_eq!(ctor_structure.kind, "constructor");
    assert_eq!(ctor_structure.name, Some("Calculator".to_string()));

    // Test method detection
    let method_structure = parsed.find_enclosing_structure(9).unwrap();
    assert_eq!(method_structure.kind, "method");
    assert_eq!(method_structure.name, Some("add".to_string()));

    // Test all top-level structures
    let all = parsed.get_all_structures();
    let kinds: Vec<&str> = all.iter().map(|s| s.kind.as_str()).collect();
    assert!(kinds.contains(&"class"));
}

#[test]
fn test_parse_java_interface_and_enum() {
    let source = r#"
public interface Drawable {
    void draw();
}

enum Color {
    RED,
    GREEN,
    BLUE;
}
"#;
    let parsed = ParsedFile::parse_with_language(source.to_string(), Language::Java).unwrap();

    // Test interface detection
    let iface_structure = parsed.find_enclosing_structure(1).unwrap();
    assert_eq!(iface_structure.kind, "interface");
    assert_eq!(iface_structure.name, Some("Drawable".to_string()));

    // Test enum detection
    let enum_structure = parsed.find_enclosing_structure(5).unwrap();
    assert_eq!(enum_structure.kind, "enum");
    assert_eq!(enum_structure.name, Some("Color".to_string()));
}

#[test]
fn test_parse_typescript_arrow_function() {
    let source = r#"
const greet = (name: string): string => {
    return `Hello, ${name}!`;
};
"#;
    let parsed = ParsedFile::parse_with_language(source.to_string(), Language::TypeScript).unwrap();

    let structure = parsed.find_enclosing_structure(2).unwrap();
    assert_eq!(structure.kind, "const");
    assert_eq!(structure.name, Some("greet".to_string()));
}

#[test]
fn test_parse_typescript_type_alias() {
    let source = r#"
type Point = {
    x: number;
    y: number;
};
"#;
    let parsed = ParsedFile::parse_with_language(source.to_string(), Language::TypeScript).unwrap();

    let structure = parsed.find_enclosing_structure(1).unwrap();
    assert_eq!(structure.kind, "type");
    assert_eq!(structure.name, Some("Point".to_string()));
}

#[test]
fn test_parse_java_record() {
    let source = r#"
public record Point(int x, int y) {
}
"#;
    let parsed = ParsedFile::parse_with_language(source.to_string(), Language::Java).unwrap();

    let structure = parsed.find_enclosing_structure(1).unwrap();
    assert_eq!(structure.kind, "record");
    assert_eq!(structure.name, Some("Point".to_string()));
}

#[test]
fn test_language_from_extension_typescript() {
    assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
    assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
    assert_eq!(Language::from_extension("mts"), Some(Language::TypeScript));
    assert_eq!(Language::from_extension("cts"), Some(Language::TypeScript));
}

#[test]
fn test_language_from_extension_java() {
    assert_eq!(Language::from_extension("java"), Some(Language::Java));
}
