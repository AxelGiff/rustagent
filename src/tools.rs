//! Native Rust Tools for agentic assistance.
//!
//! Provides ultra-fast, in-process tools for reading, writing/diffing, searching,
//! exploring directories, and executing commands, without requiring Node.js or MCP servers.

use std::path::Path;
use serde_json::{json, Value};
use crate::diff::{computed_diff, PendingDiff};

/// Result of executing a native tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Text message sent back to the model as tool output.
    pub content: String,
    /// If the tool proposed a file creation or modification, the pending diff awaiting validation.
    pub pending_diff: Option<PendingDiff>,
}

/// Returns the OpenAI-compatible function calling schemas for all native tools.
pub fn get_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Lit le contenu textuel complet d'un fichier du projet.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin relatif ou absolu du fichier à lire (ex: 'src/main.rs', 'Cargo.toml')."
                        }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Propose d'écrire ou remplacer le contenu complet d'un fichier. Cette action génère un aperçu diff et requiert la validation explicite de l'utilisateur avant d'écrire sur le disque.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin du fichier cible."
                        },
                        "content": {
                            "type": "string",
                            "description": "Nouveau contenu complet du fichier."
                        }
                    },
                    "required": ["path", "content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Remplace une section spécifique de texte existant dans un fichier par un nouveau texte. Génère un diff et requiert la validation de l'utilisateur.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin du fichier à modifier."
                        },
                        "target": {
                            "type": "string",
                            "description": "Texte exact existant à remplacer."
                        },
                        "replacement": {
                            "type": "string",
                            "description": "Nouveau texte de remplacement."
                        }
                    },
                    "required": ["path", "target", "replacement"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "Liste les fichiers et répertoires d'un dossier. IMPORTANT : Spécifiez toujours le chemin du sous-dossier ciblé (ex: './Pictures' ou './Pictures/flashcards') si vous travaillez dans un projet spécifique.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin du dossier à explorer (ex: '.' ou './Pictures/mon_projet')."
                        }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Recherche une chaîne de caractères ou motif dans les fichiers du projet.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Texte ou mot-clé à rechercher."
                        },
                        "path": {
                            "type": "string",
                            "description": "Dossier où chercher (par défaut '.')."
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Exécute une commande terminal/shell dans le projet (ex: 'python manage.py runserver', 'cargo test', 'pip install ...').",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Commande shell à exécuter."
                        },
                        "cwd": {
                            "type": "string",
                            "description": "Dossier de travail où exécuter la commande (optionnel, ex: './Pictures/flashcards')."
                        }
                    },
                    "required": ["command"]
                }
            }
        }),
    ]
}

/// Executes a native tool by name with the given JSON arguments.
pub async fn execute_native_tool(name: &str, args: Value) -> ToolResult {
    match name {
        "read_file" => {
            let path_str = args
                .get("path")
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if path_str.trim().is_empty() {
                return ToolResult {
                    content: "Erreur: argument 'path' manquant.".to_string(),
                    pending_diff: None,
                };
            }

            match std::fs::read_to_string(path_str) {
                Ok(content) => ToolResult {
                    content: truncate_output(&content, 300, 15000),
                    pending_diff: None,
                },
                Err(e) => ToolResult {
                    content: format!("Erreur lors de la lecture du fichier '{}': {}", path_str, e),
                    pending_diff: None,
                },
            }
        }

        "write_file" => {
            let path_str = args
                .get("path")
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new_content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

            if path_str.trim().is_empty() {
                return ToolResult {
                    content: "Erreur: argument 'path' manquant.".to_string(),
                    pending_diff: None,
                };
            }

            let old_content = std::fs::read_to_string(path_str).unwrap_or_default();
            let diff_lines = computed_diff(&old_content, new_content);

            let pending_diff = PendingDiff {
                id: format!(
                    "diff_{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ),
                path: path_str.to_string(),
                old_content,
                new_content: new_content.to_string(),
                diff_lines,
            };

            ToolResult {
                content: format!(
                    "Fichier '{}' préparé avec succès (diff enregistré). ACTION REQUISE : Continue immédiatement à créer ou modifier tous les autres fichiers nécessaires du projet sans t'interrompre. L'utilisateur validera l'ensemble des diffs à la fin.",
                    path_str
                ),
                pending_diff: Some(pending_diff),
            }
        }

        "edit_file" => {
            let path_str = args
                .get("path")
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let replacement = args.get("replacement").and_then(|v| v.as_str()).unwrap_or("");

            if path_str.trim().is_empty() || target.is_empty() {
                return ToolResult {
                    content: "Erreur: arguments 'path' et 'target' obligatoires pour edit_file.".to_string(),
                    pending_diff: None,
                };
            }

            let old_content = match std::fs::read_to_string(path_str) {
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
                path: path_str.to_string(),
                old_content,
                new_content,
                diff_lines,
            };

            ToolResult {
                content: format!(
                    "Modification pour '{}' préparée avec succès (diff enregistré). Continue immédiatement les étapes suivantes du projet sans t'interrompre.",
                    path_str
                ),
                pending_diff: Some(pending_diff),
            }
        }

        "list_directory" => {
            let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let path = Path::new(path_str);

            if !path.exists() {
                return ToolResult {
                    content: format!("Erreur: Le dossier '{}' n'existe pas.", path_str),
                    pending_diff: None,
                };
            }

            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let mut lines = Vec::new();
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name == "target" || name == ".git" || name == "node_modules" {
                            continue;
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
                    let content = if lines.is_empty() {
                        format!("Dossier '{}' vide.", path_str)
                    } else {
                        format!("Contenu du dossier '{}':\n{}", path_str, lines.join("\n"))
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
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let dir_str = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

            if query.trim().is_empty() {
                return ToolResult {
                    content: "Erreur: argument 'query' manquant.".to_string(),
                    pending_diff: None,
                };
            }

            let mut matches = Vec::new();
            search_dir_recursive(Path::new(dir_str), query, 0, 5, &mut matches);

            let content = if matches.is_empty() {
                format!("Aucune occurrence de '{}' trouvée dans '{}'.", query, dir_str)
            } else {
                format!("Résultats pour '{}':\n{}", query, matches.join("\n"))
            };

            ToolResult {
                content,
                pending_diff: None,
            }
        }

        "run_command" => {
            let cmd_str = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let cwd_str = args.get("cwd").and_then(|v| v.as_str());
            if cmd_str.trim().is_empty() {
                return ToolResult {
                    content: "Erreur: argument 'command' vide.".to_string(),
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

/// Recursively searches for text matches in files, skipping unwanted folders.
fn search_dir_recursive(
    dir: &Path,
    query: &str,
    depth: usize,
    max_depth: usize,
    results: &mut Vec<String>,
) {
    if depth > max_depth || results.len() >= 35 {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let query_lower = query.to_lowercase();

    for entry in entries.flatten() {
        if results.len() >= 35 {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "target" || name == ".git" || name == "node_modules" || name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        if path.is_dir() {
            search_dir_recursive(&path, query, depth + 1, max_depth, results);
        } else if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for (line_no, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&query_lower) {
                        results.push(format!("{}:{}: {}", path.display(), line_no + 1, line.trim()));
                        if results.len() >= 35 {
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Executes a shell command with a timeout.
/// Truncate text if it exceeds line or character limits to preserve context window.
pub fn truncate_output(text: &str, max_lines: usize, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let lines_truncated = if lines.len() > max_lines {
        let half = max_lines / 2;
        let head = &lines[..half];
        let tail_start = lines.len().saturating_sub(half);
        let tail = &lines[tail_start..];
        let omitted = lines.len().saturating_sub(head.len() + tail.len());
        format!(
            "{}\n\n[... {} lignes omises pour préserver le contexte ...]\n\n{}",
            head.join("\n"),
            omitted,
            tail.join("\n")
        )
    } else {
        trimmed.to_string()
    };

    if lines_truncated.len() > max_chars {
        let half_chars = max_chars / 2;
        let omitted_chars = lines_truncated.len().saturating_sub(max_chars);
        format!(
            "{}\n[... {} caractères omis ...]\n{}",
            &lines_truncated[..half_chars.min(lines_truncated.len())],
            omitted_chars,
            &lines_truncated[lines_truncated.len().saturating_sub(half_chars)..]
        )
    } else {
        lines_truncated
    }
}

/// Executes a shell command with a timeout, optionally within a working directory.
async fn run_shell_command(cmd: &str, cwd: Option<&str>) -> String {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", cmd]);
        c
    };

    if let Some(dir) = cwd {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            let path = Path::new(trimmed);
            if path.exists() {
                command.current_dir(path);
            }
        }
    }

    match tokio::time::timeout(std::time::Duration::from_secs(30), command.output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let status = output.status;
            let truncated_stdout = truncate_output(&stdout, 30, 2000);
            let truncated_stderr = truncate_output(&stderr, 30, 2000);
            format!(
                "Statut: {}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                status,
                truncated_stdout,
                truncated_stderr
            )
        }
        Ok(Err(e)) => format!("Erreur lors de l'exécution de la commande: {}", e),
        Err(_) => "Délai d'exécution dépassé (timeout 30s).".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[tokio::test]
    async fn test_tool_definitions_valid() {
        let defs = get_tool_definitions();
        assert_eq!(defs.len(), 6);
        let names: Vec<_> = defs
            .iter()
            .filter_map(|d| d.get("function")?.get("name")?.as_str())
            .collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"list_directory"));
        assert!(names.contains(&"search_files"));
        assert!(names.contains(&"run_command"));
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
}
