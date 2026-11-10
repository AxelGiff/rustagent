//! Universal file and code block extraction from tool arguments and LLM responses.

use serde_json::Value;

/// Extrait de façon ultra-tolérante tous les fichiers (chemin, contenu) d'un argument d'outil `write_file` :
/// gère le mode batch `files: [...]`, les enveloppes `parameters` / `arguments`, les tableaux directs `[{path, content}]`,
/// les dictionnaires `{ "index.html": "..." }`, les chaînes JSON stringifiées, ainsi que le mode fichier unique classique.
pub fn extract_files_from_args(args: &Value) -> Vec<(String, String)> {
    let mut result = Vec::new();

    let parse_file_obj = |obj: &serde_json::Map<String, Value>| -> Option<(String, String)> {
        let path = [
            "path",
            "file",
            "filepath",
            "file_path",
            "filename",
            "name",
            "target",
            "dest",
        ]
        .iter()
        .find_map(|k| {
            obj.get(*k)
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
        })
        .map(|s| s.trim().to_string())?;
        let content = ["content", "code", "text", "body", "source"]
            .iter()
            .find_map(|k| obj.get(*k).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        if content.trim() == "[Contenu déjà fourni au tour précédent]"
            || content.trim().starts_with("[Contenu déjà fourni")
        {
            return None;
        }
        Some((path, content))
    };

    // Cas où args est lui-même une chaîne JSON
    let mut root_val = if let Some(s) = args.as_str() {
        serde_json::from_str::<Value>(s.trim()).unwrap_or_else(|_| args.clone())
    } else {
        args.clone()
    };

    // Déballage d'éventuelles enveloppes {"parameters": ...} ou {"arguments": ...} ou {"input": ...}
    for wrap_key in &["parameters", "arguments", "input", "args"] {
        if let Some(wrapped) = root_val.get(*wrap_key)
            && (wrapped.is_object() || wrapped.is_array())
        {
            root_val = wrapped.clone();
            break;
        }
    }

    // Cas où root_val est directement un tableau
    if let Some(arr) = root_val.as_array() {
        for item in arr {
            if let Some(obj) = item.as_object()
                && let Some(pair) = parse_file_obj(obj)
            {
                result.push(pair);
            }
        }
        if !result.is_empty() {
            return result;
        }
    }

    // Cas où root_val contient un champ "files", "file_list", "sources", "items"
    if let Some(map) = root_val.as_object() {
        for key in &["files", "file_list", "sources", "items", "documents"] {
            if let Some(v) = map.get(*key) {
                match v {
                    Value::Array(arr) => {
                        for item in arr {
                            if let Some(obj) = item.as_object()
                                && let Some(pair) = parse_file_obj(obj)
                            {
                                result.push(pair);
                            }
                        }
                    }
                    Value::Object(sub_map) => {
                        for (sub_k, sub_v) in sub_map {
                            if let Some(obj) = sub_v.as_object() {
                                if let Some(pair) = parse_file_obj(obj) {
                                    result.push(pair);
                                }
                            } else if let Some(content_str) = sub_v.as_str() {
                                result.push((sub_k.clone(), content_str.to_string()));
                            }
                        }
                    }
                    Value::String(s) => {
                        if let Ok(parsed) = serde_json::from_str::<Value>(s.trim()) {
                            let sub = extract_files_from_args(&parsed);
                            if !sub.is_empty() {
                                result.extend(sub);
                            }
                        }
                    }
                    _ => {}
                }
                if !result.is_empty() {
                    return result;
                }
            }
        }

        // Cas d'un seul fichier à la racine
        if let Some(pair) = parse_file_obj(map) {
            result.push(pair);
            return result;
        }

        // Cas où l'objet racine est un dictionnaire direct "nom_fichier" -> "contenu"
        // ex: { "index.html": "<!DOCTYPE html>...", "vite.config.js": "..." }
        for (k, v) in map {
            if k.starts_with('_') {
                continue;
            }
            let is_filename = k.contains('.') || k.contains('/') || k.contains('\\');
            if is_filename {
                if let Some(content_str) = v.as_str() {
                    result.push((k.clone(), content_str.to_string()));
                } else if let Some(obj) = v.as_object()
                    && let Some(pair) = parse_file_obj(obj)
                {
                    result.push(pair);
                }
            }
        }
    }

    result
}

/// Extrait tous les blocs de code Markdown (```lang ... ```) d'un texte pour générer des fichiers.
/// Détecte le nom du fichier depuis un commentaire en tête (ex: `<!-- index.html -->` ou `// main.rs`)
/// ou déduit le nom à partir du tag de langage (ex: html -> index.html, css -> style.css, js -> script.js).
pub fn extract_code_blocks_from_text(
    text: &str,
    default_dir: Option<&str>,
) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut cursor = text;
    let mut file_index = 0;

    while let Some(start_fence) = cursor.find("```") {
        let after_fence = &cursor[start_fence + 3..];
        let Some(first_nl) = after_fence.find('\n') else {
            break;
        };
        let lang_tag = after_fence[..first_nl].trim().to_lowercase();
        let body_start = start_fence + 3 + first_nl + 1;
        let rest = &cursor[body_start..];
        let Some(end_fence) = rest.find("```") else {
            break;
        };
        let code = rest[..end_fence].trim_end().to_string();
        cursor = &rest[end_fence + 3..];

        if code.trim().is_empty() {
            continue;
        }

        file_index += 1;
        let mut detected_filename = None;
        for line in code.lines().take(3) {
            let t = line.trim();
            let candidate = if t.starts_with("<!--") && t.ends_with("-->") {
                t.trim_start_matches("<!--").trim_end_matches("-->").trim()
            } else if t.starts_with("/*") && t.ends_with("*/") {
                t.trim_start_matches("/*").trim_end_matches("*/").trim()
            } else if t.starts_with("//") {
                t.trim_start_matches("//").trim()
            } else if t.starts_with('#') {
                t.trim_start_matches('#').trim()
            } else {
                ""
            };
            let cand_clean = candidate.trim_matches(|c: char| c == '`' || c == '\'' || c == '"');
            if cand_clean.contains('.') && !cand_clean.contains(' ') && cand_clean.len() < 80 {
                detected_filename = Some(cand_clean.to_string());
                break;
            }
        }

        let filename = detected_filename.unwrap_or_else(|| match lang_tag.as_str() {
            "html" | "htm" => {
                if file_index == 1 {
                    "index.html".to_string()
                } else {
                    format!("page_{}.html", file_index)
                }
            }
            "css" => "style.css".to_string(),
            "javascript" | "js" => "script.js".to_string(),
            "typescript" | "ts" => "index.ts".to_string(),
            "python" | "py" => "main.py".to_string(),
            "rust" | "rs" => "src/main.rs".to_string(),
            "json" => format!("data_{}.json", file_index),
            _ => format!("file_{}.txt", file_index),
        });

        let target_path = if let Some(base) = default_dir {
            let clean_base = base.trim_end_matches('/');
            let clean_file = filename.trim_start_matches("./").trim_start_matches('/');
            if clean_file.starts_with(clean_base) {
                clean_file.to_string()
            } else {
                format!("{}/{}", clean_base, clean_file)
            }
        } else {
            filename
        };

        files.push((target_path, code));
    }

    files
}

/// Reads a quoted string starting at `input` (whose first character is `"` or `'`),
/// honoring `\` escapes and returning `(unescaped_string, bytes_consumed)`.
fn read_quoted_value(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (idx, ch) in chars {
        if escaped {
            match ch {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => {}
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some((out, idx + ch.len_utf8()));
        }
        out.push(ch);
    }
    // Si la chaîne est tronquée avant le guillemet fermant, on retourne quand même ce qu'on a lu
    if !out.trim().is_empty() {
        Some((out, input.len()))
    } else {
        None
    }
}

/// Extraction tolérante de fichiers depuis une chaîne brute d'arguments d'outil
/// (supporte JSON, tableaux tronqués `files: [...]`, kwargs Python style `path="...", content="..."`, ou markdown fences).
pub fn extract_files_from_raw_str(raw: &str) -> Vec<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // 1. Tenter un parsing JSON direct
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        let res = extract_files_from_args(&v);
        if !res.is_empty() {
            return res;
        }
    }

    // 2. Scanner toutes les paires ("path": "...", "content": "...") dans la chaîne brute
    let mut multi_results = Vec::new();
    let mut scan = trimmed;
    while let Some(p_idx) = ["\"path\":", "'path':", "\"file\":", "'file':"]
        .iter()
        .filter_map(|pat| scan.find(pat).map(|pos| (pos, pat.len())))
        .min_by_key(|(pos, _)| *pos)
    {
        let after_path_key = scan[p_idx.0 + p_idx.1..].trim_start();
        let Some((path_val, consumed_p)) = read_quoted_value(after_path_key) else {
            scan = &after_path_key[1.min(after_path_key.len())..];
            continue;
        };
        let rest_after_path = &after_path_key[consumed_p..];
        if let Some(c_idx) = ["\"content\":", "'content':", "\"code\":", "'code':"]
            .iter()
            .filter_map(|pat| rest_after_path.find(pat).map(|pos| (pos, pat.len())))
            .min_by_key(|(pos, _)| *pos)
        {
            let after_content_key = rest_after_path[c_idx.0 + c_idx.1..].trim_start();
            if let Some((content_val, consumed_c)) = read_quoted_value(after_content_key) {
                if !path_val.trim().is_empty() {
                    multi_results.push((path_val.trim().to_string(), content_val));
                }
                scan = &after_content_key[consumed_c..];
                continue;
            }
        }
        scan = rest_after_path;
    }

    if !multi_results.is_empty() {
        return multi_results;
    }

    // 3. Tenter d'extraire path et content via des motifs flexibles (Python kwargs, unquoted JSON)
    let mut path_opt = None;
    for key_pat in &[
        "\"path\":",
        "'path':",
        "path:",
        "path=",
        "\"file\":",
        "'file':",
        "file:",
        "file=",
    ] {
        if let Some(pos) = trimmed.find(key_pat) {
            let after = trimmed[pos + key_pat.len()..].trim_start();
            if let Some((val, _)) = read_quoted_value(after) {
                if !val.trim().is_empty() {
                    path_opt = Some(val);
                    break;
                }
            } else {
                let end = after
                    .find(|c: char| c == ',' || c == ')' || c == '}' || c.is_whitespace())
                    .unwrap_or(after.len());
                let val = after[..end].trim();
                if !val.is_empty() {
                    path_opt = Some(val.to_string());
                    break;
                }
            }
        }
    }

    let mut content_opt = None;
    for key_pat in &[
        "\"content\":",
        "'content':",
        "content:",
        "content=",
        "\"code\":",
        "'code':",
        "code:",
        "code=",
    ] {
        if let Some(pos) = trimmed.find(key_pat) {
            let after = trimmed[pos + key_pat.len()..].trim_start();
            if let Some((val, _)) = read_quoted_value(after) {
                content_opt = Some(val);
                break;
            } else {
                let end = after.rfind([')', '}']).unwrap_or(after.len());
                let val = after[..end].trim();
                if !val.is_empty() {
                    content_opt = Some(val.to_string());
                    break;
                }
            }
        }
    }

    if let (Some(p), Some(c)) = (path_opt, content_opt) {
        return vec![(p, c)];
    }

    // 4. Extraction depuis des code fences Markdown présents dans la chaîne brute
    extract_code_blocks_from_text(trimmed, None)
}

/// Tente de repérer un chemin de fichier mentionné dans un texte (ex: "le fichier `src/styles.css`")
pub fn extract_candidate_path_from_text(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        let clean = word.trim_matches(|c: char| {
            c == '`' || c == '\'' || c == '"' || c == '(' || c == ')' || c == ':' || c == ','
        });
        if clean.contains('.')
            && !clean.contains("..")
            && !clean.ends_with('.')
            && clean.len() < 100
        {
            let ext = clean.rsplit('.').next().unwrap_or("").to_lowercase();
            if matches!(
                ext.as_str(),
                "html"
                    | "htm"
                    | "css"
                    | "js"
                    | "ts"
                    | "vue"
                    | "jsx"
                    | "tsx"
                    | "json"
                    | "py"
                    | "rs"
                    | "md"
                    | "txt"
                    | "toml"
            ) {
                return Some(clean.to_string());
            }
        }
    }
    None
}
