//! Context caching layer for optimized token usage and cache hits.
//!
//! Provides:
//! - Preamble cache key/entry types for static content caching
//! - Per-turn cache statistics from API responses
//! - Session-level cache metrics tracking via global singleton
//!
//! The global [`ContextCache`] singleton (accessed via [`global_cache()`])
//! is updated after each streaming turn and reports are surfaced in the
//! `/tokens` command and per-turn usage lines.

use std::sync::{Mutex, OnceLock};

use rig::completion::Usage;

// ─────────────────────────────────────────────────────────────────────────────
// Preamble cache types (used by preamble.rs and tests)
// ─────────────────────────────────────────────────────────────────────────────

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
            Self {
                hash: hasher.finish(),
            }
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

// ─────────────────────────────────────────────────────────────────────────────
// Type definitions
// ─────────────────────────────────────────────────────────────────────────────

/// Cache statistics extracted from a single API response.
#[derive(Debug, Clone, Default)]
pub struct TurnCacheStats {
    /// Tokens served from server-side KV cache this turn
    pub cached_tokens: u64,
    /// Total input tokens this turn
    pub input_tokens: u64,
    /// Cache creation tokens (first-time prefix processing)
    pub creation_tokens: u64,
}

/// Aggregated cache metrics across all turns in a session.
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    /// Total cached input tokens this session
    pub total_cached: u64,
    /// Total input tokens this session
    pub total_input: u64,
    /// Total cache creation tokens this session
    pub total_creation: u64,
    /// Estimated USD savings from caching
    pub savings_usd: f64,
    /// Number of turns recorded
    pub turn_count: u64,
}

/// Central cache state for the application.
///
/// Tracks per-turn and session-wide cache metrics from API responses.
/// Access the singleton via [`global_cache()`].
///
/// # Usage
///
/// After each streaming turn completes, call [`record_turn`](ContextCache::record_turn)
/// with the turn's [`Usage`] from the API response. The cache instance will
/// aggregate metrics and provide formatted reports for the `/tokens` command
/// and per-turn usage display.
pub struct ContextCache {
    /// Session-wide aggregated metrics
    metrics: Mutex<CacheMetrics>,
    /// Most recent turn's cache stats (for per-turn display)
    last_turn: Mutex<Option<TurnCacheStats>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Implementations
// ─────────────────────────────────────────────────────────────────────────────

impl TurnCacheStats {
    /// Extract from API usage response.
    pub fn from_usage(usage: &Usage) -> Self {
        Self {
            cached_tokens: usage.cached_input_tokens,
            input_tokens: usage.input_tokens,
            creation_tokens: usage.cache_creation_input_tokens,
        }
    }

    /// Cache hit rate for this turn (0.0 - 1.0).
    pub fn hit_rate(&self) -> f64 {
        if self.input_tokens == 0 {
            0.0
        } else {
            self.cached_tokens as f64 / self.input_tokens as f64
        }
    }

    /// Non-cached (fresh) input tokens this turn.
    pub fn uncached_tokens(&self) -> u64 {
        self.input_tokens.saturating_sub(self.cached_tokens)
    }
}

impl CacheMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a turn's cache statistics from the API response.
    pub fn record_turn(&mut self, usage: &Usage) {
        self.total_cached += usage.cached_input_tokens;
        self.total_input += usage.input_tokens;
        self.total_creation += usage.cache_creation_input_tokens;
        // DeepSeek pricing: cached $0.028/M, uncached $0.28/M → 90% savings on cached
        self.savings_usd += usage.cached_input_tokens as f64 * 0.028 / 1_000_000.0;
        self.turn_count += 1;
    }

    /// Session-wide cache hit rate (0.0 - 1.0).
    pub fn hit_rate(&self) -> f64 {
        if self.total_input == 0 {
            0.0
        } else {
            self.total_cached as f64 / self.total_input as f64
        }
    }

    /// Format session cache statistics as lines of text.
    pub fn format_report(&self) -> Vec<String> {
        if self.turn_count == 0 {
            return vec![];
        }

        let mut lines = Vec::new();
        lines.push(String::new());
        lines.push("  ─────── Cache Stats ───────".to_string());

        let hit_rate_pct = self.hit_rate() * 100.0;
        lines.push(format!("  ◈ Hit rate: {:.1}%", hit_rate_pct));

        if self.total_cached > 0 {
            lines.push(format!("  ✓ Cached: {} tokens", self.total_cached));
        }

        if self.total_creation > 0 {
            lines.push(format!("  ⚙ Creation: {} tokens", self.total_creation));
        }

        if self.savings_usd > 0.0 {
            lines.push(format!("  💰 Savings: ${:.4}", self.savings_usd));
        }

        lines.push(format!("  📊 Turns: {}", self.turn_count));
        lines.push("  ──────────────────────────".to_string());
        lines
    }
}

// Manual Debug impl — Mutex<Option<TurnCacheStats>> doesn't auto-derive well
impl std::fmt::Debug for ContextCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextCache")
            .field("metrics", &self.metrics)
            .finish()
    }
}

impl ContextCache {
    fn new() -> Self {
        Self {
            metrics: Mutex::new(CacheMetrics::new()),
            last_turn: Mutex::new(None),
        }
    }

    /// Record cache statistics from an API response.
    ///
    /// Call this after each turn's streaming completes, before the next turn.
    pub fn record_turn(&self, usage: &Usage) {
        let stats = TurnCacheStats::from_usage(usage);
        if let Ok(mut m) = self.metrics.lock() {
            m.record_turn(usage);
        }
        if let Ok(mut last) = self.last_turn.lock() {
            *last = Some(stats);
        }
    }

    /// Get a snapshot of session-level cache metrics.
    pub fn metrics(&self) -> CacheMetrics {
        self.metrics.lock().unwrap().clone()
    }

    /// Get the most recent turn's cache statistics.
    pub fn last_turn_stats(&self) -> Option<TurnCacheStats> {
        self.last_turn.lock().unwrap().clone()
    }

    /// Format a per-turn cache hit line for display alongside turn usage.
    ///
    /// Returns `None` if no turn has been recorded yet or input tokens are zero.
    pub fn format_turn_cache_line(&self) -> Option<String> {
        let stats = self.last_turn_stats()?;
        if stats.input_tokens == 0 {
            return None;
        }
        let hit_pct = stats.hit_rate() * 100.0;
        Some(format!(
            "⛃ cache: {:.0}% · {} cached / {} input",
            hit_pct, stats.cached_tokens, stats.input_tokens
        ))
    }

    /// Format session-wide cache report (for `/tokens` command).
    pub fn format_session_report(&self) -> Vec<String> {
        self.metrics().format_report()
    }

    /// Reset all cache metrics (e.g., on session clear).
    pub fn reset(&self) {
        if let Ok(mut m) = self.metrics.lock() {
            *m = CacheMetrics::new();
        }
        if let Ok(mut last) = self.last_turn.lock() {
            *last = None;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Global singleton
// ─────────────────────────────────────────────────────────────────────────────

/// Global context cache singleton.
///
/// Initialized lazily on first access. Thread-safe.
static GLOBAL_CACHE: OnceLock<ContextCache> = OnceLock::new();

/// Get the global context cache instance.
pub fn global_cache() -> &'static ContextCache {
    GLOBAL_CACHE.get_or_init(|| ContextCache::new())
}
