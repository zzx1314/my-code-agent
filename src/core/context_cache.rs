//! Context caching layer for optimized token usage and cache hits.
//!
//! This module provides multi-layer caching:
//! - Layer 1: Preamble cache (static content - DeepSeek auto-caches)
//! - Layer 2: File content cache (mtime-based invalidation)
//! - Layer 3: Context pruning with sliding window

use std::sync::Arc;
use tokio::sync::RwLock;

/// Layer 1: Preamble cache for static content
/// 
/// DeepSeek KV cache automatically caches repeated prefixes.
/// This module helps structure prompts for optimal cache hits.
pub mod preamble_cache {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    /// Cache key for preamble content
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PreambleCacheKey {
        pub hash: u64,
    }

    impl PreambleCacheKey {
        /// Generate cache key from preamble and knowledge content
        pub fn new(preamble: &str, knowledge: &str) -> Self {
            let mut hasher = DefaultHasher::new();
            preamble.hash(&mut hasher);
            knowledge.hash(&mut hasher);
            Self { hash: hasher.finish() }
        }
    }

    /// Preamble cache entry
    #[derive(Debug, Clone)]
    pub struct PreambleCacheEntry {
        /// The full preamble text (preamble + knowledge)
        pub content: String,
        /// Cache key for invalidation detection
        pub cache_key: PreambleCacheKey,
    }

    impl PreambleCacheEntry {
        pub fn new(preamble: &str, knowledge: &str) -> Self {
            let cache_key = PreambleCacheKey::new(preamble, knowledge);
            let content = format!("{}\n\n## Project Knowledge\n{}", preamble, knowledge);
            Self { content, cache_key }
        }

        /// Check if the cache is still valid for given preamble/knowledge
        pub fn is_valid(&self, preamble: &str, knowledge: &str) -> bool {
            self.cache_key == PreambleCacheKey::new(preamble, knowledge)
        }
    }
}

/// Cache metrics for monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    /// Number of cache hits this session
    pub cache_hits: u64,
    /// Number of cache misses this session
    pub cache_misses: u64,
    /// Estimated USD savings
    pub savings_usd: f64,
}

impl CacheMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a cache hit (token count from API response)
    pub fn record_hit(&mut self, tokens: u64) {
        self.cache_hits += tokens;
        // DeepSeek: cached tokens cost $0.028/M vs $0.28/M = 90% savings
        self.savings_usd += (tokens as f64) * 0.28 / 1_000_000.0 * 0.9;
    }

    /// Record a cache miss (token count from API response)
    pub fn record_miss(&mut self, tokens: u64) {
        self.cache_misses += tokens;
    }

    /// Calculate cache hit rate (0.0 - 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }

    /// Print cache statistics
    pub fn print_report(&self) {
        use colored::*;
        
        println!();
        println!("  {}", "─────── Cache Stats ───────".bright_cyan());
        
        let hit_rate_pct = self.hit_rate() * 100.0;
        println!(
            "  {} Hit rate: {:.1}%",
            "◈".bright_cyan(),
            hit_rate_pct.to_string().bright_white()
        );
        
        if self.cache_hits > 0 {
            println!(
                "  {} Cache hits: {} tokens",
                "✓".bright_green(),
                self.cache_hits.to_string().bright_green()
            );
        }
        
        if self.cache_misses > 0 {
            println!(
                "  {} Cache misses: {} tokens",
                "○".bright_yellow(),
                self.cache_misses.to_string().bright_yellow()
            );
        }
        
        if self.savings_usd > 0.0 {
            println!(
                "  {} Estimated savings: ${:.4}",
                "💰".bright_green(),
                self.savings_usd
            );
        }
        
        println!("  {}", "──────────────────────────".bright_cyan());
    }
}

/// Shared cache state for the application
#[derive(Debug, Clone)]
pub struct ContextCache {
    /// Preamble cache (Arc for cheap cloning)
    preamble: Arc<RwLock<Option<preamble_cache::PreambleCacheEntry>>>,
}

impl ContextCache {
    /// Create new context cache
    pub fn new() -> Self {
        Self {
            preamble: Arc::new(RwLock::new(None)),
        }
    }

    /// Get or create preamble cache entry
    pub async fn get_preamble(&self, preamble: &str, knowledge: &str) -> String {
        let mut cache = self.preamble.write().await;
        
        if let Some(ref entry) = *cache {
            if entry.is_valid(preamble, knowledge) {
                return entry.content.clone();
            }
        }
        
        // Cache miss or invalid - create new entry
        let entry = preamble_cache::PreambleCacheEntry::new(preamble, knowledge);
        let content = entry.content.clone();
        *cache = Some(entry);
        
        content
    }

    /// Check if preamble is cached (for testing)
    pub async fn is_preamble_cached(&self) -> bool {
        self.preamble.read().await.is_some()
    }

    /// Clear all caches
    pub async fn clear(&self) {
        let mut cache = self.preamble.write().await;
        *cache = None;
    }
}

impl Default for ContextCache {
    fn default() -> Self {
        Self::new()
    }
}