use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;

use crate::data::{Field, Kind, LoadedData};
use crate::search::fuzzy_candidates;

#[derive(Clone)]
pub struct TgAutoDocs {
    data: Arc<LoadedData>,
}

impl TgAutoDocs {
    pub fn new(data: Arc<LoadedData>) -> Self {
        Self { data }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetSectionParams {
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetMethodParams {
    ids: OneOrMany,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetTypeParams {
    ids: OneOrMany,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    query: String,
}

#[tool_router]
impl TgAutoDocs {
    #[tool(
        description = "List the Bot API documentation sections (topics) in document order. Returns each section's id and title only, no methods or types; drill down with get_section."
    )]
    fn list_sections(&self) -> Result<CallToolResult, McpError> {
        let sections: Vec<serde_json::Value> = self
            .data
            .topics
            .iter()
            .map(|t| serde_json::json!({ "id": t.id, "title": t.title }))
            .collect();
        text_result(&serde_json::json!({ "sections": sections }))
    }

    #[tool(
        description = "Get a documentation section (topic) by id: its markdown content plus the ids of the methods and types it documents. Ids are case-insensitive; use list_sections for all section ids."
    )]
    fn get_section(
        &self,
        Parameters(p): Parameters<GetSectionParams>,
    ) -> Result<CallToolResult, McpError> {
        let key = p.id.trim().to_ascii_lowercase();
        if key.is_empty() {
            return Err(McpError::invalid_params("id must not be empty", None));
        }
        if let Some(t) = self.data.topic(&key) {
            return text_result(&serde_json::json!({
                "id": t.id,
                "kind": "topic",
                "title": t.title,
                "content_md": t.content_md,
                "methods": t.methods,
                "types": t.types,
                "url": t.url,
            }));
        }
        error_result(not_found_message(&self.data, "section", &p.id, &key))
    }

    #[tool(
        description = "Get one or more method records (sendMessage, getMe, ...). Accepts a single id or an array of ids; results come back in the requested order. Each record has fields (name, type_display, type_refs, required, description_md), returns, and url. Ids are case-insensitive; unknown ids are reported in a missing list with fuzzy suggestions, and records for the other ids are still returned."
    )]
    fn get_method(
        &self,
        Parameters(p): Parameters<GetMethodParams>,
    ) -> Result<CallToolResult, McpError> {
        let ids = match p.ids {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        };
        if ids.is_empty() {
            return Err(McpError::invalid_params("ids must not be empty", None));
        }
        let mut methods = Vec::with_capacity(ids.len());
        let mut missing = Vec::new();
        for id in &ids {
            let key = id.trim().to_ascii_lowercase();
            if key.is_empty() {
                return Err(McpError::invalid_params("ids must not be empty", None));
            }
            match self.data.method(&key) {
                Some((i, m)) => methods.push(method_json(m, &self.data.method_field_refs[i])),
                None => missing.push(not_found_message(&self.data, "method", id, &key)),
            }
        }
        if missing.is_empty() {
            text_result(&serde_json::json!({ "methods": methods }))
        } else {
            partial_result(&serde_json::json!({ "methods": methods, "missing": missing }))
        }
    }

    #[tool(
        description = "Get one or more type records (Message, InlineKeyboardMarkup, ...). Accepts a single id or an array of ids; results come back in the requested order. Each record has fields, variants for union types, the used_by reverse index, and url. Ids are case-insensitive; unknown ids are reported in a missing list with fuzzy suggestions, and records for the other ids are still returned."
    )]
    fn get_type(
        &self,
        Parameters(p): Parameters<GetTypeParams>,
    ) -> Result<CallToolResult, McpError> {
        let ids = match p.ids {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        };
        if ids.is_empty() {
            return Err(McpError::invalid_params("ids must not be empty", None));
        }
        let mut types = Vec::with_capacity(ids.len());
        let mut missing = Vec::new();
        for id in &ids {
            let key = id.trim().to_ascii_lowercase();
            if key.is_empty() {
                return Err(McpError::invalid_params("ids must not be empty", None));
            }
            match self.data.bot_type(&key) {
                Some((i, t)) => {
                    let used_by = self.data.used_by.get(&t.id).cloned().unwrap_or_default();
                    types.push(type_json(t, &self.data.type_field_refs[i], &used_by));
                }
                None => missing.push(not_found_message(&self.data, "type", id, &key)),
            }
        }
        if missing.is_empty() {
            text_result(&serde_json::json!({ "types": types }))
        } else {
            partial_result(&serde_json::json!({ "types": types, "missing": missing }))
        }
    }

    #[tool(
        description = "Search the Telegram Bot API reference with a free-text query. Returns a ranked mix of methods, types, and topics; each result has id, kind (method, type, or topic — fetch with get_method, get_type, or get_section respectively), title, and a snippet. Covers exact/prefix/fuzzy name matches, field-name reverse lookups, prose matches, and a small synonym map."
    )]
    fn search(&self, Parameters(p): Parameters<SearchParams>) -> Result<CallToolResult, McpError> {
        let q = p.query.trim();
        if q.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }
        let results: Vec<serde_json::Value> = self
            .data
            .search
            .search(q)
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "kind": r.kind,
                    "title": r.title,
                    "snippet": r.snippet,
                })
            })
            .collect();
        text_result(&serde_json::json!({ "query": q, "results": results }))
    }
}

#[tool_handler(
    name = "tgautodocs",
    version = "0.1.0",
    instructions = "Telegram Bot API reference (core.telegram.org/bots/api), machine-readable. Tools: list_sections (documentation topics), get_section (topic content), get_method (method records; one id or an array), get_type (type records with the used_by reverse index; one id or an array), search (ranked mix of methods, types, and topics). Ids are case-insensitive. Unknown ids return fuzzy suggestions alongside any records that did resolve; wrong-kind ids suggest the right getter."
)]
impl ServerHandler for TgAutoDocs {}

fn method_json(m: &crate::data::Method, refs: &[Vec<String>]) -> serde_json::Value {
    let fields: Vec<serde_json::Value> = m
        .fields
        .iter()
        .zip(refs)
        .map(|(f, r)| field_json(f, r))
        .collect();
    serde_json::json!({
        "id": m.id,
        "kind": "method",
        "description_md": m.description_md,
        "fields": fields,
        "returns": m.returns,
        "url": m.url,
    })
}

fn type_json(
    t: &crate::data::BotType,
    refs: &[Vec<String>],
    used_by: &[String],
) -> serde_json::Value {
    let fields: Vec<serde_json::Value> = t
        .fields
        .iter()
        .zip(refs)
        .map(|(f, r)| field_json(f, r))
        .collect();
    serde_json::json!({
        "id": t.id,
        "kind": "type",
        "description_md": t.description_md,
        "fields": fields,
        "variants": t.variants,
        "used_by": used_by,
        "url": t.url,
    })
}

fn field_json(f: &Field, refs: &[String]) -> serde_json::Value {
    serde_json::json!({
        "name": f.name,
        "type_display": f.type_display,
        "type_refs": refs,
        "required": f.required,
        "description_md": f.description_md,
    })
}

/// Getter tool that serves records of this kind; topics are fetched with
/// `get_section`, so the name is not derivable from `Kind::as_str`.
fn getter_tool(kind: Kind) -> &'static str {
    match kind {
        Kind::Method => "get_method",
        Kind::Type => "get_type",
        Kind::Topic => "get_section",
    }
}

fn not_found_message(data: &LoadedData, kind_name: &str, input: &str, lower: &str) -> String {
    if let Some((kind, canonical)) = data.kind_of(lower) {
        return format!(
            "{} is a {}; use {}",
            canonical,
            kind.as_str(),
            getter_tool(kind)
        );
    }
    let ids: Vec<String> = match kind_name {
        "method" => data.methods.iter().map(|m| m.id.clone()).collect(),
        "type" => data.types.iter().map(|t| t.id.clone()).collect(),
        _ => data.topics.iter().map(|t| t.id.clone()).collect(),
    };
    let suggestions = fuzzy_candidates(ids.iter().map(|s| s.as_str()), lower, 3);
    if suggestions.is_empty() {
        format!("Unknown {kind_name} '{input}'")
    } else {
        format!(
            "Unknown {kind_name} '{input}'. Did you mean: {}?",
            suggestions.join(", ")
        )
    }
}

fn text_result(value: &serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![json_block(value)?]))
}

/// Error-flagged result for a batch where some ids did not resolve: the found
/// records are kept and the unresolved ids are listed in `missing`, so one
/// miss costs one round trip instead of forcing a re-fetch of everything.
fn partial_result(value: &serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![json_block(value)?]))
}

fn json_block(value: &serde_json::Value) -> Result<ContentBlock, McpError> {
    serde_json::to_string(value)
        .map(ContentBlock::text)
        .map_err(|e| McpError::internal_error(e.to_string(), None))
}

fn error_result(message: String) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![ContentBlock::text(message)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> TgAutoDocs {
        let loaded = LoadedData::from_dataset(crate::data::fixture_dataset()).unwrap();
        TgAutoDocs::new(Arc::new(loaded))
    }

    fn text(result: &CallToolResult) -> &str {
        match result.content.first() {
            Some(ContentBlock::Text(t)) => &t.text,
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[test]
    fn getter_tool_names_match_registered_tools() {
        assert_eq!(getter_tool(Kind::Method), "get_method");
        assert_eq!(getter_tool(Kind::Type), "get_type");
        // Topics have no get_topic; sections are served by get_section.
        assert_eq!(getter_tool(Kind::Topic), "get_section");
    }

    #[test]
    fn wrong_kind_suggests_real_getter() {
        // Topic id asked of get_method must point at get_section.
        let result = tools()
            .get_method(Parameters(GetMethodParams {
                ids: OneOrMany::One("getting-updates".into()),
            }))
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(text(&result).contains("getting-updates is a topic; use get_section"));

        // Method id asked of get_section must point at get_method.
        let result = tools()
            .get_section(Parameters(GetSectionParams {
                id: "sendMessage".into(),
            }))
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(text(&result).contains("sendMessage is a method; use get_method"));
    }

    #[test]
    fn partial_batch_keeps_found_records() {
        let result = tools()
            .get_method(Parameters(GetMethodParams {
                ids: OneOrMany::Many(vec!["sendMessage".into(), "sendmesage".into()]),
            }))
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        let v: serde_json::Value = serde_json::from_str(text(&result)).unwrap();
        let methods = v["methods"].as_array().unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0]["id"], "sendMessage");
        let missing = v["missing"].as_array().unwrap();
        assert_eq!(missing.len(), 1);
        assert!(
            missing[0]
                .as_str()
                .unwrap()
                .contains("Unknown method 'sendmesage'. Did you mean: sendMessage?")
        );
    }

    #[test]
    fn partial_batch_reports_wrong_kind_in_missing() {
        let result = tools()
            .get_type(Parameters(GetTypeParams {
                ids: OneOrMany::Many(vec!["InlineKeyboardMarkup".into(), "sendMessage".into()]),
            }))
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        let v: serde_json::Value = serde_json::from_str(text(&result)).unwrap();
        let types = v["types"].as_array().unwrap();
        assert_eq!(types.len(), 1);
        assert_eq!(types[0]["id"], "InlineKeyboardMarkup");
        assert_eq!(v["missing"].as_array().unwrap().len(), 1);
        assert!(
            v["missing"][0]
                .as_str()
                .unwrap()
                .contains("sendMessage is a method; use get_method")
        );
    }

    #[test]
    fn full_batch_is_success_without_missing() {
        let result = tools()
            .get_method(Parameters(GetMethodParams {
                ids: OneOrMany::Many(vec!["sendMessage".into(), "SENDPHOTO".into()]),
            }))
            .unwrap();
        assert_eq!(result.is_error, Some(false));
        let v: serde_json::Value = serde_json::from_str(text(&result)).unwrap();
        assert_eq!(v["methods"].as_array().unwrap().len(), 2);
        assert!(v.get("missing").is_none());
    }
}
