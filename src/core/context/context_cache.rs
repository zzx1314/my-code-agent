use std::sync::{Mutex, OnceLock};

use crate::core::types::Usage;

pub mod preamble_cache {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PreambleCacheKey {
        pub hash: u64,
    }

    impl PreambleCacheKey {
        pub fn new(preamble: &str, knowledge: &str) -> Self {
            let mut hasher = DefaultHasher::new();
            preamble.hash(&mut hasher);
            knowledge.hash(&mut hasher);
            Self {
                hash: hasher.finish(),
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct PreambleCacheEntry {
        pub content: String,
        pub cache_key: PreambleCacheKey,
    }

    impl PreambleCacheEntry {
        pub fn new(preamble: &str, knowledge: &str) -> Self {
            let cache_key = PreambleCacheKey::new(preamble, knowledge);
            let content = format!("{}\n\n## Project Knowledge\n{}", preamble, knowledge);
            Self { content, cache_key }
        }

        pub fn is_valid(&self, preamble: &str, knowledge: &str) -> bool {
            self.cache_key == PreambleCacheKey::new(preamble, knowledge)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TurnCacheStats {
    pub cached_tokens: u64,
    pub input_tokens: u64,
    pub creation_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    pub total_cached: u64,
    pub total_input: u64,
    pub total_creation: u64,
    pub savings_usd: f64,
    pub turn_count: u64,
}

pub struct ContextCache {
    metrics: Mutex<CacheMetrics>,
    last_turn: Mutex<Option<TurnCacheStats>>,
}

impl TurnCacheStats {
    pub fn from_usage(usage: &Usage) -> Self {
        Self {
            cached_tokens: usage.cached_input_tokens,
            input_tokens: usage.input_tokens,
            creation_tokens: usage.cache_creation_input_tokens,
        }
    }

    pub fn hit_rate(&self) -> f64 {
        if self.input_tokens == 0 {
            0.0
        } else {
            self.cached_tokens as f64 / self.input_tokens as f64
        }
    }

    pub fn uncached_tokens(&self) -> u64 {
        self.input_tokens.saturating_sub(self.cached_tokens)
    }
}

impl CacheMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_turn(&mut self, usage: &Usage) {
        self.total_cached += usage.cached_input_tokens;
        self.total_input += usage.input_tokens;
        self.total_creation += usage.cache_creation_input_tokens;
        self.savings_usd += usage.cached_input_tokens as f64 * 0.028 / 1_000_000.0;
        self.turn_count += 1;
    }

    pub fn hit_rate(&self) -> f64 {
        if self.total_input == 0 {
            0.0
        } else {
            self.total_cached as f64 / self.total_input as f64
        }
    }

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

    pub fn record_turn(&self, usage: &Usage) {
        let stats = TurnCacheStats::from_usage(usage);
        if let Ok(mut m) = self.metrics.lock() {
            m.record_turn(usage);
        }
        if let Ok(mut last) = self.last_turn.lock() {
            *last = Some(stats);
        }
    }

    pub fn metrics(&self) -> CacheMetrics {
        self.metrics.lock().unwrap().clone()
    }

    pub fn last_turn_stats(&self) -> Option<TurnCacheStats> {
        self.last_turn.lock().unwrap().clone()
    }

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

    pub fn format_session_report(&self) -> Vec<String> {
        self.metrics().format_report()
    }

    pub fn reset(&self) {
        if let Ok(mut m) = self.metrics.lock() {
            *m = CacheMetrics::new();
        }
        if let Ok(mut last) = self.last_turn.lock() {
            *last = None;
        }
    }
}

static GLOBAL_CACHE: OnceLock<ContextCache> = OnceLock::new();

pub fn global_cache() -> &'static ContextCache {
    GLOBAL_CACHE.get_or_init(|| ContextCache::new())
}
