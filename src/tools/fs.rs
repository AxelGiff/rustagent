//! File system and process execution helpers for native tools.

use std::path::{Path, PathBuf};
use std::time::Duration;

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

    let char_count = lines_truncated.chars().count();
    if char_count > max_chars {
        let half_chars = max_chars / 2;
        let omitted_chars = char_count.saturating_sub(max_chars);
        let head_str: String = lines_truncated.chars().take(half_chars).collect();
        let tail_str: String = lines_truncated
            .chars()
            .skip(char_count.saturating_sub(half_chars))
            .collect();
        format!(
            "{}\n[... {} caractères omis ...]\n{}",
            head_str,
            omitted_chars,
            tail_str
        )
    } else {
        lines_truncated
    }
}

/// Retourne le dossier home de l'utilisateur (`HOME` ou `USERPROFILE`).
pub fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Normalise et résout intelligemment les chemins passés aux outils natifs :
/// - Convertit `~`, `~/...`, `$HOME/...`, `${HOME}/...`, `%USERPROFILE%/...` vers le vrai dossier home.
/// - Convertit les chemins MSYS2 / Git Bash `/c/Users/...` en `C:/Users/...` sous Windows.
/// - Si un chemin relatif (ex: `./Pictures/whiteboard-angular`) n'existe pas dans le dossier courant,
///   vérifie s'il existe dans le dossier home de l'utilisateur (`~/Pictures/whiteboard-angular`).
pub fn resolve_tool_path(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some(home) = user_home_dir() {
        let home_str = home.to_string_lossy().replace('\\', "/");
        if trimmed == "~" || trimmed == "$HOME" || trimmed == "${HOME}" || trimmed.eq_ignore_ascii_case("%USERPROFILE%") {
            return home_str;
        }
        for prefix in &["~/", "~\\", "$HOME/", "$HOME\\", "${HOME}/", "${HOME}\\", "%USERPROFILE%/", "%USERPROFILE%\\"] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                return format!("{}/{}", home_str.trim_end_matches('/'), rest.replace('\\', "/"));
            }
        }
    }

    let bytes = trimmed.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b'/' {
        let drive = (bytes[1] as char).to_ascii_uppercase();
        let rest = &trimmed[2..];
        return format!("{}:{}", drive, rest);
    }
    if bytes.len() == 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() {
        let drive = (bytes[1] as char).to_ascii_uppercase();
        return format!("{}:/", drive);
    }

    // Chemins Unix /Users/... fréquents avec les modèles de code sous Windows
    if trimmed.starts_with("/Users/") || trimmed.starts_with("/users/") || trimmed.starts_with("\\Users\\") || trimmed.starts_with("\\users\\") {
        let drive = user_home_dir()
            .and_then(|h| h.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()))
            .unwrap_or_else(|| "C:".to_string());
        let clean_drive = drive.trim_end_matches('\\').trim_end_matches('/');
        return format!("{}{}", clean_drive, trimmed.replace('\\', "/"));
    }

    let p = Path::new(trimmed);
    if p.exists() {
        return trimmed.to_string();
    }

    if !p.is_absolute() {
        let clean_rel = trimmed
            .trim_start_matches("./")
            .trim_start_matches(".\\")
            .replace('\\', "/");
        if !clean_rel.is_empty() && clean_rel != "." {
            if let Some(home) = user_home_dir() {
                let candidate = home.join(&clean_rel);
                if candidate.exists() {
                    return candidate.to_string_lossy().replace('\\', "/");
                }
                if let Some(parent) = candidate.parent() {
                    if parent.exists() && parent != home.as_path() {
                        return candidate.to_string_lossy().replace('\\', "/");
                    }
                    if let Some(grandparent) = parent.parent() {
                        if grandparent.exists() && grandparent != home.as_path() {
                            return candidate.to_string_lossy().replace('\\', "/");
                        }
                    }
                }
            }
        }
    }

    trimmed.to_string()
}

/// Recursively searches for file names and text matches in files, skipping unwanted folders.
pub fn search_dir_recursive(
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

    let query_lower = query.trim().trim_start_matches('*').to_lowercase();

    for entry in entries.flatten() {
        if results.len() >= 35 {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "target"
            || name == ".git"
            || name == "node_modules"
            || name == "dist"
            || name == "build"
            || name == ".angular"
            || name.starts_with('.')
        {
            continue;
        }

        let path = entry.path();
        let display_path = path.to_string_lossy().replace('\\', "/");
        let name_lower = name.to_lowercase();

        if query_lower.is_empty() {
            if path.is_dir() {
                search_dir_recursive(&path, query, depth + 1, max_depth, results);
            } else if path.is_file() {
                results.push(format!("[FILE] {}", display_path));
            }
            continue;
        }

        if name_lower.contains(&query_lower) {
            let kind = if path.is_dir() { "[DIR] " } else { "[FILE]" };
            results.push(format!("{} {}", kind, display_path));
            if results.len() >= 35 {
                break;
            }
        }

        if path.is_dir() {
            search_dir_recursive(&path, query, depth + 1, max_depth, results);
        } else if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for (line_no, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&query_lower) {
                        results.push(format!("{}:{}: {}", display_path, line_no + 1, line.trim()));
                        if results.len() >= 35 {
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Retourne la raison du refus si la commande est manifestement destructrice.
pub fn dangerous_command_reason(cmd: &str) -> Option<&'static str> {
    let lower = cmd.to_lowercase();
    let patterns: [(&str, &str); 14] = [
        ("rm -rf /", "suppression récursive de la racine"),
        ("rm -rf ~", "suppression récursive du dossier personnel"),
        ("rm -rf *", "suppression récursive globale"),
        ("del /s", "suppression récursive Windows"),
        ("rd /s", "suppression récursive Windows"),
        ("rmdir /s", "suppression récursive Windows"),
        ("remove-item -recurse", "suppression récursive PowerShell"),
        ("format c:", "formatage de disque"),
        ("mkfs", "formatage de disque"),
        ("shutdown", "arrêt de la machine"),
        ("reboot", "redémarrage de la machine"),
        ("git push --force", "réécriture de l'historique distant"),
        ("git reset --hard", "perte de modifications locales"),
        (":(){", "fork bomb"),
    ];
    patterns.iter().find(|(p, _)| lower.contains(p)).map(|(_, why)| *why)
}

/// Durée maximale d'une commande : courte pour ne jamais bloquer l'interface.
pub fn command_timeout(cmd: &str) -> Duration {
    let lower = cmd.to_lowercase();
    let slow = ["install", "build", "npm", "pip", "cargo", "npx", "startproject", "startapp", "migrate", "create"];
    if slow.iter().any(|k| lower.contains(k)) {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(8)
    }
}

/// Nettoie une commande si le modèle a accidentellement ajouté un préfixe PowerShell ou cmd.exe.
pub fn normalize_bash_command(raw: &str) -> String {
    let trimmed = raw.trim();
    for prefix in [
        "powershell -Command ",
        "powershell -command ",
        "powershell.exe -Command ",
        "cmd /C ",
        "cmd /c ",
        "cmd.exe /C ",
        "cmd.exe /c ",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let unquoted = rest.trim().trim_matches('"').trim_matches('\'');
            return unquoted.to_string();
        }
    }
    trimmed.to_string()
}

/// Executes a shell command via `bash` (Git Bash on Windows, `bash`/`sh` on Unix) with a timeout.
pub async fn run_shell_command(cmd: &str, cwd: Option<&str>) -> String {
    let clean_cmd = normalize_bash_command(cmd);
    let mut command = if let Some(bash_bin) = crate::platform::find_bash_executable() {
        let mut c = tokio::process::Command::new(bash_bin);
        c.env("TERM", "dumb");
        c.env("CI", "true");
        c.env("GIT_TERMINAL_PROMPT", "0");
        c.env("PYTHONIOENCODING", "utf-8");
        c.args(["--norc", "--noprofile", "-c", &clean_cmd]);
        c
    } else if cfg!(target_os = "windows") {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", &clean_cmd]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.env("TERM", "dumb");
        c.env("CI", "true");
        c.args(["-c", &clean_cmd]);
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

    command.stdin(std::process::Stdio::null()).kill_on_drop(true);
    let limit = command_timeout(&clean_cmd);
    match tokio::time::timeout(limit, command.output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let status = output.status;
            let truncated_stdout = truncate_output(&stdout, 30, 2000);
            let truncated_stderr = truncate_output(&stderr, 30, 2000);
            format!(
                "Statut (bash): {}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                status,
                truncated_stdout,
                truncated_stderr
            )
        }
        Ok(Err(e)) => format!("Erreur lors de l'exécution de la commande bash: {}", e),
        Err(_) => format!("Délai d'exécution dépassé (timeout {}s).", limit.as_secs()),
    }
}

