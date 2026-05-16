use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use serde_json::json;

/// A tool that allows the LLM to explicitly end its turn and return control
/// to the user, even if there are pending tool results.
#[derive(Debug, Clone)]
pub struct EndTurn;

#[async_trait::async_trait]
impl Tool for EndTurn {
    fn name(&self) -> &str {
        "end_turn"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "End your turn immediately and return control to the user. \
                 Use this when you have completed a meaningful chunk of work \
                 and want to hand control back. \
                 Do NOT use as a stop token mid-work or between tool calls."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
            }),
        }
    }

    async fn call(&self, _args: serde_json::Value) -> Result<String, String> {
        Ok(json!({"__end_turn__": true}).to_string())
    }
}
