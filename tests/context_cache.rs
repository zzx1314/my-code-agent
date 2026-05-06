use my_code_agent::core::context_cache::{
    CacheMetrics, TurnCacheStats, global_cache, preamble_cache::PreambleCacheEntry,
    preamble_cache::PreambleCacheKey,
};
use rig::completion::Usage;

#[test]
fn test_preamble_cache_key() {
    let key1 = PreambleCacheKey::new("hello", "world");
    let key2 = PreambleCacheKey::new("hello", "world");
    let key3 = PreambleCacheKey::new("hello", "other");

    assert_eq!(key1, key2);
    assert_ne!(key1, key3);
}

#[test]
fn test_preamble_cache_entry() {
    let entry = PreambleCacheEntry::new("You are a bot", "project info");

    assert!(entry.is_valid("You are a bot", "project info"));
    assert!(!entry.is_valid("You are a different bot", "project info"));
}

#[test]
fn test_turn_cache_stats() {
    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 500,
        total_tokens: 1500,
        cached_input_tokens: 800,
        cache_creation_input_tokens: 0,
    };

    let stats = TurnCacheStats::from_usage(&usage);
    assert_eq!(stats.cached_tokens, 800);
    assert_eq!(stats.input_tokens, 1000);
    assert_eq!(stats.uncached_tokens(), 200);
    assert!((stats.hit_rate() - 0.8).abs() < 0.01);
}

#[test]
fn test_cache_metrics_aggregation() {
    let mut metrics = CacheMetrics::new();

    // Turn 1: 800 cached out of 1000 input
    let usage1 = Usage {
        input_tokens: 1000,
        output_tokens: 200,
        total_tokens: 1200,
        cached_input_tokens: 800,
        cache_creation_input_tokens: 0,
    };
    metrics.record_turn(&usage1);

    // Turn 2: 500 cached out of 600 input
    let usage2 = Usage {
        input_tokens: 600,
        output_tokens: 100,
        total_tokens: 700,
        cached_input_tokens: 500,
        cache_creation_input_tokens: 0,
    };
    metrics.record_turn(&usage2);

    assert_eq!(metrics.total_cached, 1300);
    assert_eq!(metrics.total_input, 1600);
    assert_eq!(metrics.turn_count, 2);
    // Hit rate: 1300/1600 = 0.8125
    assert!((metrics.hit_rate() - 0.8125).abs() < 0.01);
    assert!(metrics.savings_usd > 0.0);
}

#[test]
fn test_cache_metrics_zero_turns() {
    let metrics = CacheMetrics::new();
    assert_eq!(metrics.hit_rate(), 0.0);
    assert!(metrics.format_report().is_empty());
}

#[test]
fn test_global_cache_record_turn() {
    let cache = global_cache();

    // Reset to clean state
    cache.reset();

    let usage = Usage {
        input_tokens: 500,
        output_tokens: 100,
        total_tokens: 600,
        cached_input_tokens: 400,
        cache_creation_input_tokens: 0,
    };
    cache.record_turn(&usage);

    let stats = cache.last_turn_stats().expect("should have turn stats");
    assert_eq!(stats.cached_tokens, 400);
    assert_eq!(stats.input_tokens, 500);

    let line = cache
        .format_turn_cache_line()
        .expect("should have cache line");
    assert!(line.contains("80%"));
}

#[test]
fn test_global_cache_format_report() {
    let cache = global_cache();
    cache.reset();

    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 200,
        total_tokens: 1200,
        cached_input_tokens: 900,
        cache_creation_input_tokens: 0,
    };
    cache.record_turn(&usage);

    let report = cache.format_session_report();
    assert!(!report.is_empty());
    // Check report contains hit rate
    let report_str = report.join("\n");
    assert!(report_str.contains("Hit rate"));
    assert!(report_str.contains("90.0%"));
}

#[test]
fn test_global_cache_reset() {
    let cache = global_cache();
    cache.reset();

    assert!(cache.last_turn_stats().is_none());
    assert!(cache.format_session_report().is_empty());
}
