//! Round 15a ─ MCP Resources / Prompts 工具定义。
//!
//! 这 4 个工具让 LLM 在 agent loop 里**主动**调用 MCP server 暴露的 resources / prompts。
//! 实际执行（dispatch）见 `tools/registry.rs`。

use crate::llm::message::ToolDefinition;
use serde_json::json;

pub fn mcp_list_resources_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "mcp_list_resources",
        "列出所有已连上 MCP server 暴露的 resources（含 server / uri / name / mimeType）。",
        json!({ "type": "object", "properties": {} }),
    )
}

pub fn mcp_read_resource_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "mcp_read_resource",
        "读一个 MCP resource 的内容。需要 server 名 + uri。",
        json!({
            "type": "object",
            "properties": {
                "server": { "type": "string", "description": "MCP server 名（mcp_servers.json 里配的）" },
                "uri":    { "type": "string", "description": "resource URI（先 mcp_list_resources 拿到）" }
            },
            "required": ["server", "uri"]
        }),
    )
}

pub fn mcp_list_prompts_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "mcp_list_prompts",
        "列出所有 MCP server 暴露的 prompt 模板（含参数定义）。",
        json!({ "type": "object", "properties": {} }),
    )
}

pub fn mcp_get_prompt_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "mcp_get_prompt",
        "取一个 prompt 模板的展开结果（多轮 messages）。",
        json!({
            "type": "object",
            "properties": {
                "server": { "type": "string" },
                "name":   { "type": "string" },
                "arguments": { "type": "object", "description": "prompt 参数（按 prompt 模板定义）" }
            },
            "required": ["server", "name"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defs_have_unique_names() {
        let all = vec![
            mcp_list_resources_definition(),
            mcp_read_resource_definition(),
            mcp_list_prompts_definition(),
            mcp_get_prompt_definition(),
        ];
        let names: Vec<&str> = all.iter().map(|d| d.function.name.as_str()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "tool name 重复: {names:?}");
    }
}
