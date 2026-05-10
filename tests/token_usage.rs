use my_code_agent::token_usage::{
    CONTEXT_WINDOW_SIZE, ContextWarning, TokenUsage, print_context_warning, print_turn_usage,
};
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
    assert_eq!(tu.context_window(), 131_072); // 128K tokens
}

#[test]
fn test_context_window_custom() {
    let tu = TokenUsage::with_context_window(1000);
    assert_eq!(tu.context_window(), 1000);
}

#[test]
fn test_context_usage_percent_zero() {
    let tu = TokenUsage::new();
    assert_eq!(tu.context_usage_percent(), 0.0);
}

#[test]
fn test_context_usage_percent_low() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(100, 50, 150));
    assert!((tu.context_usage_percent() - 10.0).abs() < 0.01); // last_turn_input 100/1000 * 100
}

#[test]
fn test_context_usage_percent_half() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(500, 100, 600));
    assert!((tu.context_usage_percent() - 50.0).abs() < 0.01); // last_turn_input 500/1000
}

#[test]
fn test_context_usage_percent_uses_last_turn_not_accumulated() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(100, 50, 150)); // turn 1: 100 input tokens
    tu.add(make_usage(200, 80, 280)); // turn 2: 200 input tokens (full prompt)
    // Should use last turn's 200, not accumulated 300
    assert!((tu.context_usage_percent() - 20.0).abs() < 0.01);
}

#[test]
fn test_context_warning_none_below_threshold() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(700, 200, 900)); // 70% last turn input usage
    assert!(tu.context_warning().is_none());
}

#[test]
fn test_context_warning_approaching_at_75() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(750, 150, 900)); // 75% last turn input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Approaching(75));
    assert_eq!(warning.threshold_percent(), 75);
}

#[test]
fn test_context_warning_approaching_at_89() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(890, 50, 940)); // 89% last turn input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Approaching(75));
}

#[test]
fn test_context_warning_critical_at_90() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(900, 100, 1000)); // 90% last turn input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Critical(90));
    assert_eq!(warning.threshold_percent(), 90);
}

#[test]
fn test_context_warning_critical_at_100() {
    let mut tu = TokenUsage::with_context_window(1000);
    tu.add(make_usage(1000, 200, 1200)); // 100% last turn input usage
    let warning = tu.context_warning().unwrap();
    assert_eq!(warning, ContextWarning::Critical(90));
}

#[test]
fn test_context_warning_zero_window_no_panic() {
    let tu = TokenUsage::with_context_window(0);
    assert_eq!(tu.context_usage_percent(), 0.0);
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

#[test]
fn test_context_usage_uses_input_tokens_only() {
    // DeepSeek (OpenAI-compatible) reports input_tokens as the full prompt length
    // including cached portions. No additional adjustment needed.
    let mut tu = TokenUsage::with_context_window(1000);
    let usage = Usage {
        input_tokens: 550,              // full prompt (includes cached parts)
        output_tokens: 50,
        total_tokens: 600,
        cached_input_tokens: 400,       // cache hit info (subset of input_tokens)
        cache_creation_input_tokens: 50,
    };
    tu.add(usage);
    // last_turn_input_tokens should be 550 (just input_tokens)
    assert_eq!(tu.last_turn_input_tokens(), 550);
    assert!((tu.context_usage_percent() - 55.0).abs() < 0.01); // 550/1000 = 55%
}

#[test]
fn test_context_usage_grows_with_more_turns() {
    // Verifies that context % grows across turns (not shrinks due to caching)
    let mut tu = TokenUsage::with_context_window(1000);

    // Turn 1: no cache
    tu.add(Usage {
        input_tokens: 500,
        output_tokens: 50,
        total_tokens: 550,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
    });
    assert_eq!(tu.last_turn_input_tokens(), 500);
    assert!((tu.context_usage_percent() - 50.0).abs() < 0.01);

    // Turn 2: 400 tokens read from cache, 200 new tokens = 600 total prompt
    // DeepSeek reports input_tokens = 600 (full prompt including cached portion)
    tu.add(Usage {
        input_tokens: 600,          // full prompt (200 new + 400 from cache)
        output_tokens: 80,
        total_tokens: 680,
        cached_input_tokens: 400,   // cache hits (subset of input_tokens)
        cache_creation_input_tokens: 0,
    });
    assert_eq!(tu.last_turn_input_tokens(), 600); // just input_tokens
    assert!((tu.context_usage_percent() - 60.0).abs() < 0.01);    // 600/1000 = 60%
}
