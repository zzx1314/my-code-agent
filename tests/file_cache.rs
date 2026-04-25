use my_code_agent::core::file_cache::FileCache;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_file_cache_new() {
    let cache = FileCache::new(10, 60);
    let stats = cache.stats();
    assert_eq!(stats.entries, 0);
}

#[test]
fn test_file_cache_insert_and_get() {
    let mut cache = FileCache::new(10, 60);

    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "line 1").unwrap();
    writeln!(file, "line 2").unwrap();
    writeln!(file, "line 3").unwrap();

    let path = file.path().to_str().unwrap();

    let (content, _entry) = cache.read_file(path, 0, 10).unwrap();
    assert!(content.contains("line 1"));

    let hit = cache.get(path);
    assert!(hit.is_some());
}

#[test]
fn test_file_cache_miss_on_modified() {
    let mut cache = FileCache::new(10, 60);

    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "original content").unwrap();

    let path = file.path().to_str().unwrap();

    let (_, _) = cache.read_file(path, 0, 100).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    let mut file2 = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file2, "new line").unwrap();

    let cached = cache.get(path);
    assert!(cached.is_none());
}

#[test]
fn test_file_cache_lru_eviction() {
    let mut cache = FileCache::new(2, 60);

    let file1 = NamedTempFile::new().unwrap();
    let file2 = NamedTempFile::new().unwrap();
    let file3 = NamedTempFile::new().unwrap();

    let path1 = file1.path().to_str().unwrap();
    let path2 = file2.path().to_str().unwrap();
    let path3 = file3.path().to_str().unwrap();

    cache.insert(path1, "content1".to_string());
    cache.insert(path2, "content2".to_string());
    cache.insert(path3, "content3".to_string());

    let stats = cache.stats();
    assert_eq!(stats.entries, 2);
}

#[test]
fn test_file_cache_invalidate() {
    let mut cache = FileCache::new(10, 60);

    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "content").unwrap();

    let path = file.path().to_str().unwrap();

    let _ = cache.read_file(path, 0, 100).unwrap();
    assert!(cache.get(path).is_some());

    cache.invalidate(path);
    assert!(cache.get(path).is_none());
}

#[test]
fn test_file_cache_clear() {
    let mut cache = FileCache::new(10, 60);

    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "content").unwrap();

    let path = file.path().to_str().unwrap();

    let _ = cache.read_file(path, 0, 100).unwrap();
    cache.clear();

    let stats = cache.stats();
    assert_eq!(stats.entries, 0);
}
