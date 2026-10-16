//! Turn intent analysis, tool selection and message hygiene for the agent loop.
//!
//! These helpers are pure or near-pure: they decide which tools to expose for a
//! given user prompt and keep the message history within the context budget.

use super::super::parser::parse_lenient_tool_args;
use super::super::types::{ChatMessage, FunctionCall, ToolCall};
use serde_json::Value;
use std::collections::BTreeMap;

/// Heuristics derived from the user prompt, system prompt and recent history,
/// used to decide which tools to expose to the model.
#[derive(Debug, Clone, Copy)]
pub struct TurnIntent {
    /// The specifications questions were already answered (skip `ask_spec_questions`).
    pub specs_already_answered: bool,
    /// The target project was pre-loaded automatically (dependencies resolved).
    pub project_preloaded: bool,
    /// The user explicitly asked for a terminal/shell command.
    pub wants_terminal_cmd: bool,
    /// The user referred to an existing folder/project.
    pub mentions_existing_folder: bool,
}

/// Extract the function name from a tool definition JSON value.
pub fn tool_name(tool: &Value) -> &str {
    tool.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
}

/// Analyse the prompt to determine the tools that make sense for this turn.
pub fn analyze_intent(
    lower_prompt: &str,
    user_prompt: &str,
    system_prompt: &str,
    history: &[crate::Message],
) -> TurnIntent {
    let specs_already_answered = user_prompt.contains("[RÉPONSES AUX SPÉCIFICATIONS]")
        || lower_prompt.contains("fais au mieux")
        || history
            .iter()
            .rev()
            .take(4)
            .any(|m| m.content.contains("SPEC_QUESTIONS_JSON:"));
    let project_preloaded = system_prompt.contains("[PROJET CIBLE PRÉ-CHARGÉ AUTOMATIQUEMENT");
    let wants_terminal_cmd = [
        "terminal", "bash", "commande", "npm ", "cargo ", "pip ", "pytest",
    ]
    .iter()
    .any(|k| lower_prompt.contains(k));
    let mentions_existing_folder = project_preloaded
        || user_prompt.contains('/')
        || user_prompt.contains('\\')
        || lower_prompt.contains("dans mon projet")
        || lower_prompt.contains("dans le dossier");

    TurnIntent {
        specs_already_answered,
        project_preloaded,
        wants_terminal_cmd,
        mentions_existing_folder,
    }
}

/// Filter the full tool list down to the tools relevant for this turn.
pub fn select_initial_tools(all: Vec<Value>, intent: &TurnIntent, spec_mode: bool) -> Vec<Value> {
    all.into_iter()
        .filter(|t| {
            let name = tool_name(t);
            if (!spec_mode || intent.specs_already_answered) && name == "ask_spec_questions" {
                return false;
            }
            if intent.specs_already_answered && name == "write_plan" {
                return false;
            }
            if !intent.wants_terminal_cmd && name == "run_command" {
                return false;
            }
            if (intent.project_preloaded
                || !intent.mentions_existing_folder
                || intent.specs_already_answered)
                && (name == "list_directory" || name == "search_files")
            {
                return false;
            }
            if !intent.mentions_existing_folder && name == "read_file" {
                return false;
            }
            true
        })
        .collect()
}

/// Restrict the exposed tools once the exploration budget is exhausted, forcing
/// the model to move on to writing/editing.
pub fn active_tools_for_iteration(
    tools_payload: &[Value],
    allow_tools: bool,
    consecutive_read_turns: usize,
    total_read_calls: usize,
    list_dir_calls: usize,
    max_read_turns: usize,
    max_total_read_calls: usize,
) -> Option<Vec<Value>> {
    if !allow_tools {
        return None;
    }
    if consecutive_read_turns >= max_read_turns || total_read_calls >= max_total_read_calls {
        Some(
            tools_payload
                .iter()
                .filter(|t| {
                    matches!(
                        tool_name(t),
                        "write_file"
                            | "write_plan"
                            | "edit_file"
                            | "ask_spec_questions"
                            | "verify_project"
                    )
                })
                .cloned()
                .collect(),
        )
    } else if list_dir_calls >= 1 {
        Some(
            tools_payload
                .iter()
                .filter(|t| tool_name(t) != "list_directory")
                .cloned()
                .collect(),
        )
    } else {
        Some(tools_payload.to_vec())
    }
}

/// Truncate large tool outputs from older turns to keep the context window small.
pub fn truncate_old_tool_outputs(messages: &mut [ChatMessage]) {
    let total_msgs = messages.len();
    if total_msgs > 8 {
        for msg in messages.iter_mut().take(total_msgs.saturating_sub(4)) {
            if msg.role == "tool"
                && let Some(content) = &mut msg.content
                && content.len() > 1000
            {
                *content = crate::tools::truncate_output(content, 20, 800);
            }
        }
    }
}

/// Whether the user asked for a single, small file (as opposed to a whole project).
pub fn is_single_file_intent(lower_prompt: &str) -> bool {
    !lower_prompt.contains("projet")
        && !lower_prompt.contains("plusieurs")
        && !lower_prompt.contains("application")
        && !lower_prompt.contains("composant")
        && (lower_prompt.contains("un simple")
            || lower_prompt.contains("un seul")
            || lower_prompt.contains("un fichier")
            || lower_prompt.contains("le fichier")
            || lower_prompt.contains("index.html")
            || lower_prompt.contains("page html")
            || lower_prompt.contains("fichier html")
            || lower_prompt.contains("simple html")
            || lower_prompt.contains("script ")
            || (!lower_prompt.contains(" et ")
                && (lower_prompt.ends_with(".html")
                    || lower_prompt.ends_with(".rs")
                    || lower_prompt.ends_with(".py")
                    || lower_prompt.ends_with(".js")
                    || lower_prompt.ends_with(".css"))))
}

/// Turn the accumulated streaming tool-call fragments into clean [`ToolCall`]s.
pub fn finalize_tool_calls(
    tool_calls: &BTreeMap<usize, (String, String, String)>,
    iteration: usize,
) -> Vec<ToolCall> {
    let mut calls_vec = Vec::new();
    for (idx, (id, name, args)) in tool_calls.values().enumerate() {
        if name.trim().is_empty() {
            continue;
        }
        let call_id = if id.is_empty() || id == "0" || id == "call_0" {
            format!("call_{}_{}", iteration, idx)
        } else {
            id.clone()
        };
        let clean_args = if let Ok(val) = serde_json::from_str::<Value>(args) {
            val.to_string()
        } else {
            parse_lenient_tool_args(args).to_string()
        };
        calls_vec.push(ToolCall {
            id: call_id,
            call_type: "function".to_string(),
            function: FunctionCall {
                name: name.clone(),
                arguments: clean_args,
            },
        });
    }
    calls_vec
}
