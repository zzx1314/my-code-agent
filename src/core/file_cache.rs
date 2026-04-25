use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct FileCacheEntry {
    content: String,
    mtime: SystemTime,
    size: u64,
    lines: usize,
}

pub struct FileCache {
    entries: HashMap<PathBuf, FileCacheEntry>,
    access_order: VecDeque<PathBuf>,
    max_entries: usize,
    max_age: Duration,
}

impl FileCache {
    pub fn new(max_entries: usize, max_age_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            access_order: VecDeque::new(),
            max_entries,
            max_age: Duration::from_secs(max_age_secs),
        }
    }

    pub fn get(&mut self, path: &str) -> Option<FileCacheEntry> {
        let path = PathBuf::from(path);
        
        if let Some(entry) = self.entries.get(&path) {
            let is_valid = self.is_entry_valid(&path, entry);
            if is_valid {
                let entry_clone = entry.clone();
                self.update_access(&path);
                return Some(entry_clone);
            }
        }
        
        None
    }

    pub fn insert(&mut self, path: &str, content: String) {
        let path = PathBuf::from(path);
        let size = content.len() as u64;
        let lines = content.lines().count();
        
        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        
        if self.entries.len() >= self.max_entries {
            self.evict_lru();
        }
        
        let entry = FileCacheEntry { content, mtime, size, lines };
        self.entries.insert(path.clone(), entry);
        self.update_access(&path);
    }

    pub fn read_file(&mut self, path: &str, offset: usize, limit: usize) -> Option<(String, FileCacheEntry)> {
        if let Some(entry) = self.get(path) {
            let slice = entry.content
                .lines()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>()
                .join("\n");
            return Some((slice, entry));
        }
        
        let content = std::fs::read_to_string(path).ok()?;
        
        self.insert(path, content.clone());
        self.get(path).map(|entry| {
            let slice = entry.content
                .lines()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>()
                .join("\n");
            (slice, entry)
        })
    }

    fn is_entry_valid(&self, path: &PathBuf, entry: &FileCacheEntry) -> bool {
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Ok(current_mtime) = metadata.modified() {
                return current_mtime == entry.mtime;
            }
        }
        false
    }

    fn update_access(&mut self, path: &PathBuf) {
        self.access_order.retain(|p| p != path);
        self.access_order.push_front(path.clone());
    }

    fn evict_lru(&mut self) {
        if let Some(lru) = self.access_order.pop_back() {
            self.entries.remove(&lru);
        }
    }

    pub fn invalidate(&mut self, path: &str) {
        let path = PathBuf::from(path);
        self.entries.remove(&path);
        self.access_order.retain(|p| p != &path);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
    }

    pub fn stats(&self) -> FileCacheStats {
        let total_size: u64 = self.entries.values().map(|e| e.size).sum();
        FileCacheStats {
            entries: self.entries.len(),
            total_bytes: total_size,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileCacheStats {
    pub entries: usize,
    pub total_bytes: u64,
}