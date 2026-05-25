use crate::core::agent::client::LlmClient;
use crate::core::config::Config;
use crate::core::types::Message;

/// Default fast model for translation.
const DEFAULT_TRANSLATION_MODEL: &str = "deepseek-v4-flash";

/// Resolve a provider's default base URL (matching `build_client` in preamble.rs).
fn provider_default_url(provider: &str) -> &'static str {
    match provider {
        "deepseek" => "https://api.deepseek.com/v1",
        "openrouter" => "https://openrouter.ai/api/v1",
        "ollama" => "http://localhost:11434/v1",
        _ => "https://api.deepseek.com/v1",
    }
}

/// Resolve a provider's default API key env-var name.
fn provider_default_api_key_env(provider: &str) -> &'static str {
    match provider {
        "deepseek" => "DEEPSEEK_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "custom" => "OPENAI_API_KEY",
        "ollama" => "OLLAMA_API_KEY",
        _ => "DEEPSEEK_API_KEY",
    }
}

/// Resolve the model name for translation.
///
/// Priority:
/// 1. `config.translation.model` (if non-empty)
/// 2. Main LLM's model (for custom providers where we don't know model names)
/// 3. Default: `"deepseek-v4-flash"` (or `"deepseek/deepseek-v4-flash"` for OpenRouter)
fn resolve_model(config: &Config) -> String {
    let cfg = &config.translation;
    if !cfg.model.is_empty() {
        return cfg.model.clone();
    }
    // For custom providers, fall back to the main LLM model (we don't know
    // what models are available on the custom endpoint).
    if let Some(ref model) = config.llm.model {
        return model.clone();
    }
    if config.llm.provider == "openrouter" {
        format!("deepseek/{}", DEFAULT_TRANSLATION_MODEL)
    } else {
        DEFAULT_TRANSLATION_MODEL.to_string()
    }
}

/// Resolve the base URL for translation.
///
/// Priority:
/// 1. `config.translation.base_url` (if set)
/// 2. Main provider's base_url override (for custom provider)
/// 3. Provider default URL
fn resolve_base_url(config: &Config) -> String {
    let cfg = &config.translation;
    if let Some(ref url) = cfg.base_url {
        return url.clone();
    }
    if let Some(ref url) = config.llm.base_url {
        return url.clone();
    }
    provider_default_url(&config.llm.provider).to_string()
}

/// Resolve the API key for translation.
///
/// Priority:
/// 1. `config.translation.api_key_env` (if non-empty)
/// 2. Main LLM's `api_key_env`
/// 3. Provider default env var name
fn resolve_api_key(config: &Config) -> String {
    let cfg = &config.translation;
    let env_var = if !cfg.api_key_env.is_empty() {
        &cfg.api_key_env
    } else if !config.llm.api_key_env.is_empty() {
        &config.llm.api_key_env
    } else {
        provider_default_api_key_env(&config.llm.provider)
    };
    std::env::var(env_var).unwrap_or_default()
}

/// Resolve the timeout for translation (default: 15 seconds).
fn resolve_timeout(config: &Config) -> u64 {
    let cfg = &config.translation;
    if cfg.timeout_secs > 0 {
        cfg.timeout_secs
    } else {
        15
    }
}

/// Returns true if the input text contains any CJK (Chinese) characters.
pub fn contains_chinese(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(
            c,
            '\u{4E00}'..='\u{9FFF}'
                | '\u{3400}'..='\u{4DBF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{2F800}'..='\u{2FA1F}'
        )
    })
}

/// Returns whether auto-translation is enabled in config.
pub fn is_enabled(config: &Config) -> bool {
    config.translation.enabled
}

/// Build a dedicated LLM client for translation from config.
pub fn build_client(config: &Config) -> LlmClient {
    let base_url = resolve_base_url(config);
    let model = resolve_model(config);

    // For local Ollama, skip API key auth entirely.
    // Check the resolved base_url directly — this covers both:
    // - Main provider is Ollama triggering localhost default
    // - Translation explicitly configured to use local Ollama (regardless of main provider)
    let is_local_ollama = base_url.starts_with("http://localhost:11434")
        || base_url.starts_with("http://127.0.0.1:11434");

    let api_key = if is_local_ollama {
        tracing::info!("Translation: local Ollama — skipping API key auth");
        String::new()
    } else {
        resolve_api_key(config)
    };

    let timeout = resolve_timeout(config);
    LlmClient::new(&base_url, &api_key, &model)
        .with_timeout(timeout)
        .with_reasoning_disabled(config.translation.disable_reasoning)
}

/// Translate Chinese text to English using a dedicated lightweight LLM client.
///
/// Always uses its own client built from `config.translation` — independent
/// from the main agent's model/provider.
///
/// Returns the translated text on success, or the original text on failure.
pub async fn translate_chinese_to_english(config: &Config, text: &str) -> String {
    let client = build_client(config);

    let messages = vec![
        Message::system(
            "You are a precise translator. Translate the given Chinese text to English. \
             Output ONLY the translation — no notes, no explanations, no quotes, no prefixes.",
        ),
        Message::user(text.to_string()),
    ];

    match client.chat(&messages, &[], "").await {
        Ok(response) => {
            let translated = response["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or(text);
            let trimmed = translated.trim();
            if trimmed.is_empty() {
                text.to_string()
            } else {
                trimmed.to_string()
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Translation failed, falling back to original text");
            text.to_string()
        }
    }
}
