//! Common domain types and message models for RustAgent.

use crate::tools;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    AI,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterViewMode {
    Editor,
    Diff,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SpecQuestionItem {
    #[serde(default)]
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub security_note: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SpecQuestionsPayload {
    #[serde(default)]
    pub context_summary: Option<String>,
    #[serde(default)]
    pub questions: Vec<SpecQuestionItem>,
}

/// Extrait le texte Markdown propre, l'indicateur `[RECOMMEND_SPEC_MODE]`, et les questions
/// interactives `SPEC_QUESTIONS_JSON:{...}` d'un message de l'assistant.
pub fn extract_spec_payload(content: &str) -> (String, bool, Option<SpecQuestionsPayload>) {
    let formatted_content = tools::format_thinking_in_text(content);
    let recommend_spec = formatted_content.contains("[RECOMMEND_SPEC_MODE]");
    let mut cleaned = formatted_content.replace("[RECOMMEND_SPEC_MODE]", "");
    let mut parsed_payload = None;

    if let Some(pos) = cleaned.find("SPEC_QUESTIONS_JSON:") {
        let before = cleaned[..pos].trim_end().to_string();
        let after = cleaned[pos + "SPEC_QUESTIONS_JSON:".len()..].trim();
        let json_str = if let Some(newline_pos) = after.find('\n') {
            after[..newline_pos].trim()
        } else {
            after
        };
        if let Ok(mut payload) = serde_json::from_str::<SpecQuestionsPayload>(json_str) {
            if payload.questions.is_empty() {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let norm = tools::normalize_spec_questions(&val, "");
                    if let Ok(q) = serde_json::from_value::<Vec<SpecQuestionItem>>(norm) {
                        payload.questions = q;
                    }
                }
            }
            parsed_payload = Some(payload);
        } else if let Ok(questions) = serde_json::from_str::<Vec<SpecQuestionItem>>(json_str) {
            let q_vec = if questions.is_empty() {
                let norm = tools::fallback_spec_questions("");
                serde_json::from_value::<Vec<SpecQuestionItem>>(serde_json::json!(norm)).unwrap_or_default()
            } else {
                questions
            };
            parsed_payload = Some(SpecQuestionsPayload {
                context_summary: None,
                questions: q_vec,
            });
        } else if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
            let norm = tools::normalize_spec_questions(&val, "");
            if let Ok(questions) = serde_json::from_value::<Vec<SpecQuestionItem>>(norm) {
                parsed_payload = Some(SpecQuestionsPayload {
                    context_summary: None,
                    questions,
                });
            }
        }
        cleaned = before;
    }

    (cleaned.trim().to_string(), recommend_spec, parsed_payload)
}

