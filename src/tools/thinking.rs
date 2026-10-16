//! Reasoning / Thinking tag filtering and streaming formatters.

/// Supprime les balises `<think>...</think>` et tout leur contenu interne.
/// Utilisé pour nettoyer les messages assistant envoyés dans l'historique API.
pub fn strip_thinking_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = text;
    while let Some(start) = cursor.find("<think>") {
        result.push_str(&cursor[..start]);
        if let Some(end) = cursor[start..].find("</think>") {
            cursor = &cursor[start + end + "</think>".len()..];
        } else {
            // Balise <think> non fermée (en cours de stream ou tronquée)
            cursor = "";
            break;
        }
    }
    result.push_str(cursor);
    result
}

/// Remplace les blocs `<think>...</think>` par un bloc Markdown visuel discret
/// pour afficher que l'IA a réfléchi tout en masquant le contenu brut du monologue.
pub fn format_thinking_in_text(text: &str) -> String {
    let mut cleaned = text.to_string();
    while let Some(start) = cleaned.find("<think>") {
        if let Some(end) = cleaned[start..].find("</think>") {
            cleaned.replace_range(
                start..start + end + "</think>".len(),
                "> 💭 *Réflexion de l'assistant (masquée)*\n\n",
            );
        } else {
            cleaned.replace_range(start.., "> 💭 *Réflexion en cours...*\n\n");
            break;
        }
    }
    cleaned
}

/// Filtre de streaming en temps réel pour masquer le contenu de pensée `<think>...</think>`
/// et afficher à la place un visuel `💭 *Réflexion en cours...*`.
#[derive(Debug, Clone)]
pub struct ThinkingFilter {
    pub inside_think: bool,
    pub announced: bool,
    pending: String,
    pub accumulated_thinking: String,
    pub clean_content: String,
}

impl Default for ThinkingFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl ThinkingFilter {
    pub fn new() -> Self {
        Self {
            inside_think: false,
            announced: false,
            pending: String::new(),
            accumulated_thinking: String::new(),
            clean_content: String::new(),
        }
    }

    /// Signale et stocke du reasoning direct issu de `delta.reasoning_content`
    pub fn ingest_reasoning(&mut self, reasoning: &str) -> Option<&'static str> {
        if reasoning.is_empty() {
            return None;
        }
        self.accumulated_thinking.push_str(reasoning);
        if !self.announced {
            self.announced = true;
            Some("💭 *Réflexion en cours...*\n\n")
        } else {
            None
        }
    }

    /// Traite un morceau de texte `delta.content` et retourne le texte sûr à afficher à l'utilisateur.
    pub fn process_chunk(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let mut to_emit = String::new();

        loop {
            if !self.inside_think {
                if let Some(pos) = self.pending.find("<think>") {
                    let before = self.pending[..pos].to_string();
                    if !before.is_empty() {
                        self.clean_content.push_str(&before);
                        to_emit.push_str(&before);
                    }
                    self.inside_think = true;
                    if !self.announced {
                        self.announced = true;
                        to_emit.push_str("💭 *Réflexion en cours...*\n\n");
                    }
                    self.pending = self.pending[pos + "<think>".len()..].to_string();
                } else {
                    let mut partial_len = 0;
                    for len in (1.."<think>".len()).rev() {
                        if self.pending.as_bytes().ends_with(&b"<think>"[..len]) {
                            partial_len = len;
                            break;
                        }
                    }
                    let emit_len = self.pending.len().saturating_sub(partial_len);
                    if emit_len > 0 && self.pending.is_char_boundary(emit_len) {
                        let text = self.pending[..emit_len].to_string();
                        self.clean_content.push_str(&text);
                        to_emit.push_str(&text);
                        self.pending = self.pending[emit_len..].to_string();
                    }
                    break;
                }
            } else {
                if let Some(pos) = self.pending.find("</think>") {
                    let think_part = self.pending[..pos].to_string();
                    self.accumulated_thinking.push_str(&think_part);
                    self.inside_think = false;
                    self.pending = self.pending[pos + "</think>".len()..].to_string();
                } else {
                    let mut partial_len = 0;
                    for len in (1.."</think>".len()).rev() {
                        if self.pending.as_bytes().ends_with(&b"</think>"[..len]) {
                            partial_len = len;
                            break;
                        }
                    }
                    let absorb_len = self.pending.len().saturating_sub(partial_len);
                    if absorb_len > 0 && self.pending.is_char_boundary(absorb_len) {
                        let text = self.pending[..absorb_len].to_string();
                        self.accumulated_thinking.push_str(&text);
                        self.pending = self.pending[absorb_len..].to_string();
                    }
                    break;
                }
            }
        }

        to_emit
    }

    /// Finalise le flux et émet le reste de texte s'il n'est pas dans `<think>`
    pub fn finalize(&mut self) -> String {
        let mut to_emit = String::new();
        if self.inside_think {
            self.accumulated_thinking.push_str(&self.pending);
            self.pending.clear();
        } else if !self.pending.is_empty() {
            let p = std::mem::take(&mut self.pending);
            self.clean_content.push_str(&p);
            to_emit.push_str(&p);
        }
        to_emit
    }
}
