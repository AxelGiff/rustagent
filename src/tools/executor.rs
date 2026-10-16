//! Dispatch router and executor for native tools.

use std::path::Path;
use serde_json::Value;
use crate::diff::{computed_diff, PendingDiff};
use super::definitions::ToolResult;
use super::extract::{extract_files_from_args, extract_files_from_raw_str};
use super::fs::{dangerous_command_reason, resolve_tool_path, run_shell_command, search_dir_recursive, truncate_output};
use super::plan::{ensure_plan_in_gitignore, normalize_spec_questions};

/// Executes a native tool by name with the given JSON arguments.
pub async fn execute_native_tool(name: &str, args: Value) -> ToolResult {
    match name {
        "read_file" => {
            let raw_path = args
                .get("path")
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let path_str = resolve_tool_path(raw_path);

            if path_str.trim().is_empty() {
                return ToolResult {
                    content: "Erreur: argument 'path' manquant.".to_string(),
                    pending_diff: None,
                };
            }

            match std::fs::read_to_string(&path_str) {
                Ok(content) => ToolResult {
                    content: truncate_output(&content, 120, 6000),
                    pending_diff: None,
                },
                Err(e) => ToolResult {
                    content: format!("Erreur lors de la lecture du fichier '{}': {}", path_str, e),
                    pending_diff: None,
                },
            }
        }

        "ask_spec_questions" => {
            let user_hint = args
                .get("_user_hint")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let payload = normalize_spec_questions(&args, user_hint);
            let serialized = serde_json::to_string(&payload).unwrap_or_else(|_| "[]".to_string());
            ToolResult {
                content: format!("SPEC_QUESTIONS_DISPLAYED:{}", serialized),
                pending_diff: None,
            }
        }

        "write_plan" => {
            let raw_path = args
                .get("path")
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("plan.md");
            let resolved_raw = resolve_tool_path(raw_path);
            let path_str = if resolved_raw.trim().is_empty() {
                "plan.md".to_string()
            } else if resolved_raw.ends_with("plan.md") {
                resolved_raw
            } else {
                let p = Path::new(&resolved_raw);
                p.join("plan.md").to_string_lossy().replace('\\', "/")
            };
            let new_content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

            let p = Path::new(&path_str);
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            let gitignore_updated = ensure_plan_in_gitignore(p);
            let _ = std::fs::write(p, new_content);

            let gitignore_note = if gitignore_updated {
                " (ajouté automatiquement au .gitignore)"
            } else {
                ""
            };

            ToolResult {
                content: format!(
                    "OK: Plan d'architecture et Security by Design enregistré dans '{}'{} ! Tu as maintenant toutes les spécifications en main : passe IMMÉDIATEMENT à l'implémentation du code avec write_file / edit_file.",
                    path_str, gitignore_note
                ),
                pending_diff: None,
            }
        }

        "write_file" => {
            let mut files = extract_files_from_args(&args);
            if files.is_empty() {
                if let Some(s) = args.as_str() {
                    files = extract_files_from_raw_str(s);
                } else if let Ok(s) = serde_json::to_string(&args) {
                    files = extract_files_from_raw_str(&s);
                }
            }
            let (target_raw_path, new_content_str) = if let Some((p, c)) = files.first() {
                (p.as_str(), c.as_str())
            } else {
                let p = ["path", "file", "filepath", "file_path", "filename", "name", "target", "dest"]
                    .iter()
                    .find_map(|k| args.get(*k).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()))
                    .unwrap_or("");
                let c = ["content", "code", "text", "body", "source"]
                    .iter()
                    .find_map(|k| args.get(*k).and_then(|v| v.as_str()))
                    .unwrap_or("");
                (p, c)
            };

            let path_str = resolve_tool_path(target_raw_path);

            if path_str.trim().is_empty() {
                let serialized = serde_json::to_string(&args).unwrap_or_default();
                return ToolResult {
                    content: format!("Erreur: argument 'path' manquant. Args reçus: {}", truncate_output(&serialized, 2, 80)),
                    pending_diff: None,
                };
            }

            let clean_content = new_content_str.trim();
            if clean_content == "[Contenu déjà fourni au tour précédent]"
                || clean_content.starts_with("[Contenu déjà fourni")
            {
                return ToolResult {
                    content: format!("⚠️ Erreur: Le contenu envoyé pour '{}' est un placeholder fictif ('[Contenu déjà fourni au tour précédent]'). Tu DOIS impérativement fournir le code source RÉEL et COMPLET du fichier dans l'argument 'content' !", path_str),
                    pending_diff: None,
                };
            }

            let target_path = Path::new(&path_str);
            ensure_plan_in_gitignore(target_path);

            let old_content = std::fs::read_to_string(&path_str).unwrap_or_default();
            let diff_lines = computed_diff(&old_content, new_content_str);

            let pending_diff = PendingDiff {
                id: format!(
                    "diff_{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ),
                path: path_str.clone(),
                old_content,
                new_content: new_content_str.to_string(),
                diff_lines,
            };

            ToolResult {
                content: format!("OK: '{}' préparé (diff généré). Écris maintenant le prochain fichier manquant pour le projet (ex: composants, styles, scripts), ou termine avec ta conclusion textuelle si tous les fichiers sont prêts.", path_str),
                pending_diff: Some(pending_diff),
            }
        }

        "edit_file" => {
            let raw_path = args
                .get("path")
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let path_str = resolve_tool_path(raw_path);
            let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let replacement = args.get("replacement").and_then(|v| v.as_str()).unwrap_or("");

            if path_str.trim().is_empty() || target.is_empty() {
                return ToolResult {
                    content: "Erreur: arguments 'path' et 'target' obligatoires pour edit_file.".to_string(),
                    pending_diff: None,
                };
            }

            let old_content = match std::fs::read_to_string(&path_str) {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult {
                        content: format!("Erreur lors de la lecture de '{}': {}", path_str, e),
                        pending_diff: None,
                    };
                }
            };

            if !old_content.contains(target) {
                return ToolResult {
                    content: format!(
                        "Erreur: Le motif cible 'target' n'a pas été trouvé dans '{}'. Assurez-vous que l'extrait correspond exactement.",
                        path_str
                    ),
                    pending_diff: None,
                };
            }

            let new_content = old_content.replacen(target, replacement, 1);
            let diff_lines = computed_diff(&old_content, &new_content);

            let pending_diff = PendingDiff {
                id: format!(
                    "diff_{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ),
                path: path_str.clone(),
                old_content,
                new_content,
                diff_lines,
            };

            ToolResult {
                content: format!("OK: '{}' modifié (diff généré). Poursuis les étapes suivantes.", path_str),
                pending_diff: Some(pending_diff),
            }
        }

        "list_directory" => {
            let raw_path = args
                .get("path")
                .or_else(|| args.get("dir"))
                .or_else(|| args.get("directory"))
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let path_str = resolve_tool_path(raw_path);
            let filter_str = args
                .get("filter")
                .or_else(|| args.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_lowercase();
            let path = Path::new(&path_str);

            if !path.exists() {
                return ToolResult {
                    content: format!("Erreur: Le dossier '{}' n'existe pas.", path_str),
                    pending_diff: None,
                };
            }

            let filter_tokens: Vec<String> = filter_str
                .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
                .map(|t| t.trim().trim_start_matches('*').to_string())
                .filter(|t| !t.is_empty())
                .collect();

            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let mut lines = Vec::new();
                    const IGNORED_NAMES: &[&str] = &[
                        "target",
                        ".git",
                        "node_modules",
                        ".venv",
                        "venv",
                        "__pycache__",
                        "dist",
                        "build",
                        ".next",
                        ".nuxt",
                        ".angular",
                        ".cache",
                        "coverage",
                        ".idea",
                        ".vscode",
                        ".rustagent",
                        "Cargo.lock",
                        "package-lock.json",
                        "pnpm-lock.yaml",
                        "yarn.lock",
                    ];

                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if IGNORED_NAMES.iter().any(|ig| ig.eq_ignore_ascii_case(&name)) {
                            continue;
                        }
                        if !filter_tokens.is_empty() {
                            let name_lower = name.to_lowercase();
                            if !filter_tokens.iter().any(|tok| name_lower.contains(tok)) {
                                continue;
                            }
                        }
                        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        if is_dir {
                            lines.push(format!("[DIR]  {}/", name));
                        } else {
                            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            lines.push(format!("[FILE] {} ({} octets)", name, size));
                        }
                    }
                    lines.sort();
                    if lines.len() > 35 {
                        let total = lines.len();
                        lines.truncate(35);
                        lines.push(format!("... ({} autres éléments omis — utilisez 'filter' ou 'search_files')", total - 35));
                    }
                    let content = if lines.is_empty() {
                        if !filter_tokens.is_empty() {
                            format!("Aucun fichier correspondant au filtre '{}' dans '{}'.", filter_str, path_str)
                        } else {
                            format!("Dossier '{}' vide.", path_str)
                        }
                    } else {
                        format!(
                            "Contenu ciblé de '{}':\n{}\n\n💡 Rappel : Lis uniquement le fichier nécessaire à la fonctionnalité ou passe immédiatement à `write_file` / `edit_file`.",
                            path_str,
                            lines.join("\n")
                        )
                    };
                    ToolResult {
                        content,
                        pending_diff: None,
                    }
                }
                Err(e) => ToolResult {
                    content: format!("Erreur de lecture du dossier '{}': {}", path_str, e),
                    pending_diff: None,
                },
            }
        }

        "search_files" => {
            let query = args
                .get("query")
                .or_else(|| args.get("pattern"))
                .or_else(|| args.get("filter"))
                .or_else(|| args.get("name"))
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let raw_dir = args
                .get("path")
                .or_else(|| args.get("dir"))
                .or_else(|| args.get("directory"))
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let dir_str = resolve_tool_path(raw_dir);
            let dir_path = Path::new(&dir_str);

            if !dir_path.exists() {
                return ToolResult {
                    content: format!("Erreur: Le dossier '{}' n'existe pas.", dir_str),
                    pending_diff: None,
                };
            }

            let mut matches = Vec::new();
            search_dir_recursive(dir_path, query, 0, 5, &mut matches);

            let label = if query.is_empty() { "*" } else { query };
            let content = if matches.is_empty() {
                format!("Aucune occurrence de '{}' trouvée dans '{}'.", label, dir_str)
            } else {
                format!("Résultats pour '{}' dans '{}':\n{}", label, dir_str, matches.join("\n"))
            };

            ToolResult {
                content,
                pending_diff: None,
            }
        }

        "run_command" => {
            let cmd_str = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let resolved_cwd = args
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(resolve_tool_path);
            let cwd_str = resolved_cwd.as_deref();
            if cmd_str.trim().is_empty() {
                return ToolResult {
                    content: "Erreur: argument 'command' vide.".to_string(),
                    pending_diff: None,
                };
            }
            if let Some(reason) = dangerous_command_reason(cmd_str) {
                return ToolResult {
                    content: format!(
                        "Erreur: commande refusée par les garde-fous ({}). Propose une alternative plus sûre ou demande à l'utilisateur de l'exécuter lui-même.",
                        reason
                    ),
                    pending_diff: None,
                };
            }

            let output = run_shell_command(cmd_str, cwd_str).await;
            ToolResult {
                content: output,
                pending_diff: None,
            }
        }

        unknown => ToolResult {
            content: format!("Outil inconnu '{}'.", unknown),
            pending_diff: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use super::super::definitions::get_tool_definitions;
    use super::super::fs::dangerous_command_reason;
    use super::super::thinking::{format_thinking_in_text, strip_thinking_tags, ThinkingFilter};

    #[test]
    fn test_dangerous_commands_blocked() {
        assert!(dangerous_command_reason("rm -rf /").is_some());
        assert!(dangerous_command_reason("git reset --hard HEAD").is_some());
        assert!(dangerous_command_reason("cargo test").is_none());
        assert!(dangerous_command_reason("pip install django").is_none());
    }

    #[test]
    fn test_truncate_output() {
        let short = "Line 1\nLine 2";
        assert_eq!(truncate_output(short, 10, 100), short);

        let long_lines = (1..=50).map(|i| format!("Line {}", i)).collect::<Vec<_>>().join("\n");
        let truncated = truncate_output(&long_lines, 10, 10000);
        assert!(truncated.contains("omises pour préserver le contexte"));
        assert!(truncated.contains("Line 1"));
        assert!(truncated.contains("Line 50"));

        let huge_text = "a".repeat(5000);
        let truncated_chars = truncate_output(&huge_text, 100, 200);
        assert!(truncated_chars.len() < 1000);
        assert!(truncated_chars.contains("caractères omis"));

        let utf8_text = "éàè💡⚠️🛡️".repeat(60);
        for max_c in 1..50 {
            let res = truncate_output(&utf8_text, 10, max_c);
            assert!(!res.is_empty());
        }
    }

    #[tokio::test]
    async fn test_tool_definitions_valid() {
        let defs = get_tool_definitions();
        assert_eq!(defs.len(), 8);
        let names: Vec<_> = defs
            .iter()
            .filter_map(|d| d.get("function")?.get("name")?.as_str())
            .collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"write_plan"));
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"list_directory"));
        assert!(names.contains(&"search_files"));
        assert!(names.contains(&"run_command"));
        assert!(names.contains(&"ask_spec_questions"));
    }

    #[tokio::test]
    async fn test_read_file_existing() {
        let res = execute_native_tool("read_file", json!({ "path": "Cargo.toml" })).await;
        assert!(res.content.contains("[package]"));
        assert!(res.pending_diff.is_none());
    }

    #[tokio::test]
    async fn test_write_file_proposes_diff() {
        let res = execute_native_tool("write_file", json!({ "path": "test_temp.txt", "content": "hello world\n" })).await;
        assert!(res.pending_diff.is_some());
        let diff = res.pending_diff.unwrap();
        assert_eq!(diff.path, "test_temp.txt");
        assert_eq!(diff.new_content, "hello world\n");
    }

    #[tokio::test]
    async fn test_edit_file_target_replacement() {
        let temp_path = std::env::temp_dir().join("test_edit_file.txt");
        std::fs::write(&temp_path, "let x = 1;\nlet y = 2;\n").unwrap();

        let res = execute_native_tool(
            "edit_file",
            json!({
                "path": temp_path.to_str().unwrap(),
                "target": "let y = 2;",
                "replacement": "let y = 42;"
            }),
        )
        .await;

        assert!(res.pending_diff.is_some());
        let diff = res.pending_diff.unwrap();
        assert!(diff.new_content.contains("let y = 42;"));
        let _ = std::fs::remove_file(temp_path);
    }

    #[tokio::test]
    async fn test_list_directory() {
        let res = execute_native_tool("list_directory", json!({ "path": "src" })).await;
        assert!(res.content.contains("main.rs"));
        assert!(res.pending_diff.is_none());
    }

    #[tokio::test]
    async fn test_run_command() {
        let cmd = if cfg!(target_os = "windows") { "echo hello" } else { "echo hello" };
        let res = execute_native_tool("run_command", json!({ "command": cmd })).await;
        assert!(res.content.contains("hello"));
    }

    #[tokio::test]
    async fn test_write_plan_auto_updates_gitignore() {
        let temp_dir = std::env::temp_dir().join("rustagent_test_plan_gitignore");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let gitignore_path = temp_dir.join(".gitignore");
        std::fs::write(&gitignore_path, "target/\nnode_modules/\n").unwrap();

        let plan_path = temp_dir.join("plan.md");
        let res = execute_native_tool(
            "write_plan",
            json!({
                "path": plan_path.to_str().unwrap(),
                "content": "# Plan Security by Design\n- Auth JWT\n"
            }),
        )
        .await;

        assert!(res.content.contains("OK:"));
        assert!(plan_path.exists());
        let updated_gitignore = std::fs::read_to_string(&gitignore_path).unwrap();
        assert!(updated_gitignore.contains("plan.md"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_ask_spec_questions_returns_marker() {
        let res = execute_native_tool(
            "ask_spec_questions",
            json!({
                "context_summary": "Choix d'authentification",
                "questions": [
                    {
                        "id": "auth",
                        "question": "Quel mécanisme d'authentification souhaites-tu ?",
                        "options": ["(Recommandé) JWT HttpOnly + CSRF", "Session serveur Redis"],
                        "security_note": "Protection contre le vol de token XSS"
                    }
                ]
            }),
        )
        .await;
        assert!(res.content.starts_with("SPEC_QUESTIONS_DISPLAYED:"));
        assert!(res.content.contains("JWT HttpOnly"));
    }

    #[test]
    fn test_strip_thinking_tags() {
        let raw = "<think>\nThinking about life...\nAnd code...\n</think>Hello world!";
        let stripped = strip_thinking_tags(raw);
        assert_eq!(stripped, "Hello world!");

        let unclosed = "<think>Still thinking...";
        assert_eq!(strip_thinking_tags(unclosed), "");
    }

    #[test]
    fn test_format_thinking_in_text() {
        let raw = "<think>Monologue secret</think>Voici le code final";
        let formatted = format_thinking_in_text(raw);
        assert!(formatted.contains("💭 *Réflexion de l'assistant (masquée)*"));
        assert!(formatted.contains("Voici le code final"));
        assert!(!formatted.contains("Monologue secret"));
    }

    #[test]
    fn test_thinking_filter_streaming() {
        let mut filter = ThinkingFilter::new();
        let chunk1 = filter.process_chunk("<think>Je réfl");
        assert_eq!(chunk1, "💭 *Réflexion en cours...*\n\n");

        let chunk2 = filter.process_chunk("échis au fichier HTML</think>Voici le fichier :");
        assert_eq!(chunk2, "Voici le fichier :");

        let end = filter.finalize();
        assert_eq!(end, "");
        assert_eq!(filter.clean_content, "Voici le fichier :");
        assert_eq!(filter.accumulated_thinking, "Je réfléchis au fichier HTML");
    }

    #[test]
    fn test_thinking_filter_streaming_utf8() {
        let mut filter = ThinkingFilter::new();
        let chunk1 = filter.process_chunk("Voici un menu concoté avec café et thé : ");
        assert_eq!(chunk1, "Voici un menu concoté avec café et thé : ");
        let chunk2 = filter.process_chunk("<think>Réflexion sur les crêpes</think>Crêpes prêtes !");
        assert!(chunk2.contains("Crêpes prêtes !"));
        assert!(!chunk2.contains("Réflexion"));
    }

    #[test]
    fn test_write_file_definition_schema() {
        let defs = get_tool_definitions();
        let write_file_def = defs.iter().find(|t| {
            t.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                == Some("write_file")
        });
        assert!(write_file_def.is_some());
        let def = write_file_def.unwrap();
        let properties = &def["function"]["parameters"]["properties"];
        assert!(properties.get("files").is_some());
        assert!(properties.get("path").is_some());
        assert!(properties.get("content").is_some());
    }
}

