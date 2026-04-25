use my_code_agent::core::context_cache::{CacheMetrics, preamble_cache::PreambleCacheEntry, preamble_cache::PreambleCacheKey};

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
fn test_cache_metrics() {
    let mut metrics = CacheMetrics::new();
    
    metrics.record_hit(1000);
    metrics.record_miss(2000);
    
    assert_eq!(metrics.cache_hits, 1000);
    assert_eq!(metrics.cache_misses, 2000);
    assert!((metrics.hit_rate() - 0.333).abs() < 0.01);
}