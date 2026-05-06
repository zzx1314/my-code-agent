use my_code_agent::core::parser::ParsedFile;

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
