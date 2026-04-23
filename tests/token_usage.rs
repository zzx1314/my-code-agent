use my_code_agent::token_usage::{TokenUsage, print_turn_usage};
use rig::completion::Usage;

fn make_usage(input: u64, output: u64, total: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
    }
}

#[test]
fn test_new_is_zero() {
    let tu = TokenUsage::new();
    let u = tu.usage();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn test_default_is_zero() {
    let tu = TokenUsage::default();
    let u = tu.usage();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn test_add_single_turn() {
    let mut tu = TokenUsage::new();
    tu.add(make_usage(100, 50, 150));
    let u = tu.usage();
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn test_add_multiple_turns() {
    let mut tu = TokenUsage::new();
    tu.add(make_usage(100, 50, 150));
    tu.add(make_usage(200, 80, 280));
    let u = tu.usage();
    assert_eq!(u.input_tokens, 300);
    assert_eq!(u.output_tokens, 130);
    assert_eq!(u.total_tokens, 430);
}

#[test]
fn test_add_with_cached_tokens() {
    let mut tu = TokenUsage::new();
    let usage_with_cache = Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: 40,
        cache_creation_input_tokens: 10,
    };
    tu.add(usage_with_cache);
    let u = tu.usage();
    assert_eq!(u.cached_input_tokens, 40);
    assert_eq!(u.cache_creation_input_tokens, 10);
}

#[test]
fn test_add_accumulates_cached_tokens() {
    let mut tu = TokenUsage::new();
    let first = Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: 40,
        cache_creation_input_tokens: 10,
    };
    let second = Usage {
        input_tokens: 200,
        output_tokens: 80,
        total_tokens: 280,
        cached_input_tokens: 60,
        cache_creation_input_tokens: 20,
    };
    tu.add(first);
    tu.add(second);
    let u = tu.usage();
    assert_eq!(u.cached_input_tokens, 100);
    assert_eq!(u.cache_creation_input_tokens, 30);
}

#[test]
fn test_print_turn_usage_does_not_panic() {
    let usage = make_usage(100, 50, 150);
    // Just verify it doesn't panic; output goes to stdout
    print_turn_usage(&usage);
}
