//! Chat history builders, tool argument sanitizers, and lenient JSON repair.

use serde_json::{json, Value};
use crate::types::{Message, Role};

/// Build a list of `rig::completion::Message` context history items from UI messages,
/// skipping initial system warnings/welcome messages and capping to `max_history` entries.
pub fn build_chat_history(messages: &[Message], max_history: usize) -> Vec<rig::completion::Message> {
    let mut history = Vec::new();

    for msg in messages {
        if msg.role == Role::AI
            && (msg.content.starts_with("Hello! I'm your coding assistant")
                || msg.content.starts_with("Chat cleared")
                || msg.content.starts_with("⚠️ "))
        {
            continue;
        }

        match msg.role {
            Role::User => {
                if !msg.content.trim().is_empty() {
                    history.push(rig::completion::Message::user(msg.content.clone()));
                }
            }
            Role::AI => {
                if !msg.content.trim().is_empty() {
                    history.push(rig::completion::Message::assistant(msg.content.clone()));
                }
            }
        }
    }

    if history.len() > max_history {
        history.drain(0..history.len() - max_history);
    }

    history
}

/// Résume les arguments d'un appel d'outil pour l'affichage (chemin, commande, requête).
pub fn describe_tool_call(name: &str, args: &Value) -> String {
    let pick = |keys: &[&str]| -> String {
        keys.iter()
            .find_map(|k| args.get(*k).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()))
            .unwrap_or("")
            .chars()
            .take(80)
            .collect()
    };
    match name {
        "read_file" | "write_file" | "write_plan" | "edit_file" | "list_directory" => {
            let raw = pick(&["path", "file", "dir", "directory"]);
            if raw.is_empty() {
                raw
            } else {
                crate::tools::resolve_tool_path(&raw)
            }
        }
        "search_files" => pick(&["query", "pattern", "filter", "name", "path"]),
        "run_command" => pick(&["command"]),
        "ask_spec_questions" => "Questions de cadrage Security by Design".to_string(),
        _ => String::new(),
    }
}

/// Analyse et répare de façon tolérante les arguments JSON d'un appel d'outil provenant du flux SSE :
/// 1. Retire les markdown code fences (` ```json `).
/// 2. Retire les préfixes `{}` doublés parfois envoyés par les serveurs vLLM / Albert.
/// 3. Échappe les caractères de contrôle (vrais sauts de ligne `\n`, tabulations) à l'intérieur des chaînes JSON,
///    très fréquents lorsque les modèles LLM comme DeepSeek génèrent du code dans les arguments d'outils.
/// 4. Auto-ferme les guillemets et accolades manquants si le flux a été tronqué.
/// 5. Extrait par motif de secours `path` et `content` en dernier recours.
pub fn parse_lenient_tool_args(raw: &str) -> Value {
    let mut s = raw.trim();
    if s.is_empty() {
        return json!({});
    }

    if s.starts_with("```") {
        if let Some(first_newline) = s.find('\n') {
            s = &s[first_newline + 1..];
        }
        if let Some(last_fence) = s.rfind("```") {
            s = &s[..last_fence];
        }
        s = s.trim();
    }

    while s.starts_with("{}{") {
        s = &s[2..];
    }

    // 1. Essai direct standard
    if let Ok(val) = serde_json::from_str::<Value>(s) {
        return val;
    }

    // 2. Réparation des sauts de ligne et tabulations brutes à l'intérieur des chaînes JSON
    let mut sanitized = String::with_capacity(s.len() + 128);
    let mut in_str = false;
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            sanitized.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                sanitized.push(ch);
                escaped = true;
            }
            '"' => {
                in_str = !in_str;
                sanitized.push(ch);
            }
            '\n' if in_str => {
                sanitized.push_str("\\n");
            }
            '\r' if in_str => {}
            '\t' if in_str => {
                sanitized.push_str("\\t");
            }
            _ => {
                sanitized.push(ch);
            }
        }
    }

    if let Ok(val) = serde_json::from_str::<Value>(&sanitized) {
        return val;
    }

    // 3. Auto-fermeture des guillemets et accolades tronqués
    let mut auto_closed = sanitized.clone();
    let mut open_braces: i32 = 0;
    let mut open_brackets: i32 = 0;
    let mut inside_quote = false;
    let mut esc = false;
    for ch in auto_closed.chars() {
        if esc {
            esc = false;
            continue;
        }
        match ch {
            '\\' => esc = true,
            '"' => inside_quote = !inside_quote,
            '{' if !inside_quote => open_braces += 1,
            '}' if !inside_quote && open_braces > 0 => open_braces -= 1,
            '[' if !inside_quote => open_brackets += 1,
            ']' if !inside_quote && open_brackets > 0 => open_brackets -= 1,
            _ => {}
        }
    }
    if inside_quote {
        auto_closed.push('"');
    }
    while open_brackets > 0 {
        auto_closed.push(']');
        open_brackets -= 1;
    }
    while open_braces > 0 {
        auto_closed.push('}');
        open_braces -= 1;
    }

    if let Ok(val) = serde_json::from_str::<Value>(&auto_closed) {
        return val;
    }

    if let Some(val) = serde_json::Deserializer::from_str(&auto_closed)
        .into_iter::<Value>()
        .filter_map(|r| r.ok())
        .last()
    {
        return val;
    }

    // 4. Utilisation de l'extracteur universel de fichiers
    let raw_files = crate::tools::extract_files_from_raw_str(raw);
    if !raw_files.is_empty() {
        if raw_files.len() == 1 {
            let (p, c) = &raw_files[0];
            return json!({ "path": p, "content": c });
        } else {
            let arr: Vec<Value> = raw_files
                .into_iter()
                .map(|(p, c)| json!({ "path": p, "content": c }))
                .collect();
            return json!({ "files": arr });
        }
    }

    // 5. Extraction heuristique de secours pour write_file (path + content)
    let mut extracted = json!({});
    for key in &["path", "file", "filepath", "file_path", "filename", "name"] {
        let pattern = format!("\"{}\":", key);
        if let Some(pos) = s.find(&pattern) {
            let after = s[pos + pattern.len()..].trim();
            if after.starts_with('"') {
                if let Some(end_quote) = after[1..].find('"') {
                    let path_val = &after[1..1 + end_quote];
                    extracted[key] = json!(path_val);
                    break;
                }
            }
        }
    }
    for key in &["content", "code", "text", "body", "source"] {
        let pattern = format!("\"{}\":", key);
        if let Some(pos) = s.find(&pattern) {
            let mut after = s[pos + pattern.len()..].trim();
            if after.starts_with('"') {
                after = &after[1..];
                let end_pos = after.rfind('"').unwrap_or(after.len());
                let content_val = &after[..end_pos];
                extracted[key] = json!(content_val.replace("\\n", "\n").replace("\\\"", "\""));
                break;
            }
        }
    }

    if extracted.get("path").is_some() || extracted.get("file").is_some() {
        return extracted;
    }

    json!({})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_chat_history_filters_system_welcome_and_orders_messages() {
        let msgs = vec![
            Message {
                role: Role::AI,
                content: "Hello! I'm your coding assistant...".to_string(),
            },
            Message {
                role: Role::User,
                content: "write python code".to_string(),
            },
            Message {
                role: Role::AI,
                content: "def foo(): pass".to_string(),
            },
        ];

        let history = build_chat_history(&msgs, 20);
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn build_chat_history_caps_length_to_max() {
        let mut msgs = Vec::new();
        for i in 0..30 {
            msgs.push(Message {
                role: if i % 2 == 0 { Role::User } else { Role::AI },
                content: format!("Message {}", i),
            });
        }

        let history = build_chat_history(&msgs, 10);
        assert_eq!(history.len(), 10);
    }
}

