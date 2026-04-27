use my_code_agent::core::config::Config;
use my_code_agent::mcp::web_search_tool::{ParallelWebSearch, ParallelWebFetch};

#[tokio::test]
async fn test_parallel_web_search_no_api_key() {
    let search = ParallelWebSearch::new("");
    assert!(search.is_available());
}

#[tokio::test]
async fn test_parallel_web_search_with_api_key() {
    let search = ParallelWebSearch::new("test_key_123");
    assert!(search.is_available());
}

#[tokio::test]
async fn test_parallel_web_fetch_no_api_key() {
    let fetch = ParallelWebFetch::new("");
    assert!(fetch.is_available());
}

#[tokio::test]
async fn test_mcp_config_parallel_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[mcp]\nenabled = true\nparallel_api_key = \"my_key\"\n",
    )
    .unwrap();

    let config = Config::load_from(&path);
    assert!(config.mcp.enabled);
    assert_eq!(config.mcp.parallel_api_key, Some("my_key".to_string()));
}