use crate::core::agent::client::LlmClient;
use crate::core::types::ToolDefinition;
use crate::tools::Tool;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpawnAgentInput {
    pub agent_type: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpawnAgentResult {
    pub agent_type: String,
    pub prompt: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct SpawnAgents {
    client: LlmClient,
    reasoning_field: String,
}

impl SpawnAgents {
    pub fn new(client: LlmClient, reasoning_field: String) -> Self {
        Self { client, reasoning_field }
    }

    fn system_prompt_for_type(agent_type: &str) -> String {
        match agent_type {
            "reviewer" => {
                "You are a code review specialist. Analyze code changes, identify bugs, security issues, \
                performance problems, and suggest improvements. Be concise and focus on high-impact issues. \
                Provide concrete fix examples where possible."
            }
            "researcher" => {
                "You are a technical research specialist. When given a topic or question, provide \
                comprehensive, well-structured answers with relevant examples, best practices, and \
                current information. Cite sources when applicable."
            }
            "coder" => {
                "You are a senior software engineer. Write clean, well-structured code that follows \
                best practices for the target language. Include error handling, comments, and tests \
                where appropriate."
            }
            "summarizer" => {
                "You are a technical summarization specialist. Condense complex information into \
                clear, concise summaries. Preserve key decisions, file paths, and important context. \
                Use bullet points for readability."
            }
            "security" => {
                "You are a security specialist. Analyze code for vulnerabilities including SQL injection, \
                XSS, CSRF, authentication bypass, authorization flaws, sensitive data exposure, and \
                insecure dependencies. Provide specific remediation steps."
            }
            _ => {
                "You are a helpful AI assistant. Provide clear, accurate, and well-structured responses. \
                When analyzing code, focus on correctness, readability, and best practices."
            }
        }
        .to_string()
    }
}

#[async_trait::async_trait]
impl Tool for SpawnAgents {
    fn name(&self) -> &str {
        "spawn_agents"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Spawn multiple sub-agents to run tasks in parallel. Each agent has a specific \
                type (reviewer, researcher, coder, summarizer, security) and receives its own prompt. \
                All agents run concurrently and results are combined. \
                Available agent types: reviewer (code review), researcher (technical research), \
                coder (code generation), summarizer (text summarization), security (security audit), \
                or any custom type (uses generic assistant prompt)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agents": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent_type": {
                                    "type": "string",
                                    "description": "Type of agent to spawn: reviewer, researcher, coder, summarizer, security, or any custom type"
                                },
                                "prompt": {
                                    "type": "string",
                                    "description": "The prompt/task to send to this agent"
                                }
                            },
                            "required": ["agent_type", "prompt"]
                        },
                        "description": "List of agents to spawn in parallel"
                    }
                },
                "required": ["agents"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        #[derive(Deserialize)]
        struct SpawnAgentsArgs {
            agents: Vec<SpawnAgentInput>,
        }

        let args: SpawnAgentsArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;

        if args.agents.is_empty() {
            return Err("No agents specified. Provide at least one agent with agent_type and prompt.".to_string());
        }

        if args.agents.len() > 10 {
            return Err("Too many agents (max 10). Reduce the number of parallel agents.".to_string());
        }

        let tasks: Vec<_> = args
            .agents
            .into_iter()
            .map(|agent| {
                let client = self.client.clone();
                async move {
                    let system_prompt = Self::system_prompt_for_type(&agent.agent_type);
                    let messages = vec![
                        Message::system(system_prompt),
                        Message::user(agent.prompt.clone()),
                    ];

                    match client.chat(&messages, &[], &self.reasoning_field).await {
                        Ok(response) => {
                            let content = response["choices"][0]["message"]["content"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            SpawnAgentResult {
                                agent_type: agent.agent_type.clone(),
                                prompt: agent.prompt.clone(),
                                content,
                                error: None,
                            }
                        }
                        Err(e) => SpawnAgentResult {
                            agent_type: agent.agent_type.clone(),
                            prompt: agent.prompt.clone(),
                            content: String::new(),
                            error: Some(e.to_string()),
                        },
                    }
                }
            })
            .collect();

        let results = join_all(tasks).await;

        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }
}

use crate::core::types::Message;
