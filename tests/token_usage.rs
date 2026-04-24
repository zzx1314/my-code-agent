use my_code_agent::token_usage::{ContextWarning, TokenUsage, print_context_warning, print_turn_usage, CONTEXT_WINDOW_SIZE};
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

// ── Context window tests ──

#[test]
fn test_context_window_default() {
    let tu = TokenUsage::new();
    assert_eq!(tu.context_window(), CONTEXT_WINDOW_SIZE);
    assert_eq!(tu.context_window(), 65_536);
}

#[test]
fn test_context_window_custom() {
    let tu = TokenUsage::with_context_window(1000);
    assert_eq!(tu.context_window(), 1000);
}

#[test]
fn test_context_usage_percent_zero() {
    let tu = TokenUsage::new();
    assert_eq!(tu.context_usage_percent(), 0);
}

#[test]
fn test_context_usage_percent_low() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(100, 50, 150));
    assert_eq!(tu.context_usage_percent(), 10); // 100/1000 * 100 (input_tokens only)
}

#[test]
fn test_context_usage_percent_half() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(500, 100, 600));
    assert_eq!(tu.context_usage_percent(), 50); // 500/1000 (input_tokens only)
}

#[test]
fn test_context_warning_none_below_threshold() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(700, 200, 900)); // 70% input usage
    assert!(tu.context_warning().is_none());
}

#[test]
fn test_context_warning_approaching_at_75() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(750, 150, 900)); // 75% input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Approaching);
    assert_eq!(warning.threshold_percent(), 75);
}

#[test]
fn test_context_warning_approaching_at_89() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(890, 50, 940)); // 89% input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Approaching);
}

#[test]
fn test_context_warning_critical_at_90() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(900, 100, 1000)); // 90% input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Critical);
    assert_eq!(warning.threshold_percent(), 90);
}

#[test]
fn test_context_warning_critical_at_100() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(1000, 200, 1200)); // 100% input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Critical);
}

#[test]
fn test_context_warning_zero_window_no_panic() {
    let tu = TokenUsage::with_context_window(0);
    assert_eq!(tu.context_usage_percent(), 0);
    assert!(tu.context_warning().is_none());
}

#[test]
fn test_input_output_total_tokens_accessors() {
    let mut tu = TokenUsage::new();
    tu.add(make_usage(100, 50, 150));
    assert_eq!(tu.input_tokens(), 100);
    assert_eq!(tu.output_tokens(), 50);
    assert_eq!(tu.total_tokens(), 150);
}

#[test]
fn test_print_context_warning_no_panic() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(900, 100, 1000)); // 90% input usage - critical
    // Just verify it doesn't panic
    print_context_warning(&tu);
}

#[test]
fn test_print_context_warning_none_no_panic() {
    let tu = TokenUsage::new();
    // No warning at 0%
    print_context_warning(&tu);
}

#[test]
fn test_print_session_report_does_not_panic() {
    let mut tu = TokenUsage::new();
    tu.add(make_usage(100, 50, 150));
    tu.print_session_report();
}
