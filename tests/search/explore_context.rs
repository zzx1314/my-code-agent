use my_code_agent::tools::search::{RawMatch, build_clusters, extract_terms, lang_tag};

#[test]
fn test_extract_terms_filters_common_words() {
    let terms = extract_terms("how does authentication work with token");
    assert!(!terms.iter().any(|t| t == "how"));
    assert!(!terms.iter().any(|t| t == "does"));
    assert!(terms.iter().any(|t| t == "authentication"));
    assert!(terms.iter().any(|t| t == "token"));
}

#[test]
fn test_extract_terms_keeps_identifiers() {
    let terms = extract_terms("AuthService validateToken session_manager");
    assert!(terms.iter().any(|t| t == "AuthService"));
    assert!(terms.iter().any(|t| t == "validateToken"));
    assert!(terms.iter().any(|t| t == "session_manager"));
}

#[test]
fn test_build_clusters_merges_nearby() {
    let ms = vec![
        RawMatch {
            file: "x.rs".into(),
            line: 10,
        },
        RawMatch {
            file: "x.rs".into(),
            line: 12,
        },
        RawMatch {
            file: "x.rs".into(),
            line: 50,
        },
    ];
    let clusters = build_clusters(&ms);
    assert_eq!(clusters.len(), 2);
    assert_eq!(clusters[0].start, 10);
    assert_eq!(clusters[0].end, 12);
    assert_eq!(clusters[1].start, 50);
    assert_eq!(clusters[1].end, 50);
}

#[test]
fn test_lang_tag() {
    assert_eq!(lang_tag("foo.rs"), "rust");
    assert_eq!(lang_tag("bar.ts"), "typescript");
    assert_eq!(lang_tag("baz.py"), "python");
    assert_eq!(lang_tag("qux.unknown"), "text");
}
