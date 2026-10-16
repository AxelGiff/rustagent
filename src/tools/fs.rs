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
            head_str, omitted_chars, tail_str
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
        if trimmed == "~"
            || trimmed == "$HOME"
            || trimmed == "${HOME}"
            || trimmed.eq_ignore_ascii_case("%USERPROFILE%")
        {
            return home_str;
        }
        for prefix in &[
            "~/",
            "~\\",
            "$HOME/",
            "$HOME\\",
            "${HOME}/",
            "${HOME}\\",
            "%USERPROFILE%/",
            "%USERPROFILE%\\",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                return format!(
                    "{}/{}",
                    home_str.trim_end_matches('/'),
                    rest.replace('\\', "/")
                );
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
    if trimmed.starts_with("/Users/")
        || trimmed.starts_with("/users/")
        || trimmed.starts_with("\\Users\\")
        || trimmed.starts_with("\\users\\")
    {
        let drive = user_home_dir()
            .and_then(|h| {
                h.components()
                    .next()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
            })
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
        if !clean_rel.is_empty()
            && clean_rel != "."
            && let Some(home) = user_home_dir()
        {
            let candidate = home.join(&clean_rel);
            if candidate.exists() {
                return candidate.to_string_lossy().replace('\\', "/");
            }
            if let Some(parent) = candidate.parent() {
                if parent.exists() && parent != home.as_path() {
                    return candidate.to_string_lossy().replace('\\', "/");
                }
                if let Some(grandparent) = parent.parent()
                    && grandparent.exists()
                    && grandparent != home.as_path()
                {
                    return candidate.to_string_lossy().replace('\\', "/");
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
        } else if path.is_file()
            && let Ok(content) = std::fs::read_to_string(&path)
        {
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

/// Retourne la raison du refus si la commande est manifestement destructrice.
pub fn dangerous_command_reason(cmd: &str) -> Option<&'static str> {
    let lower = cmd.to_lowercase();
    let patterns: [(&str, &str); 19] = [
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
        (
            "| sh",
            "exécution de code téléchargé via un pipe vers un shell",
        ),
        (
            "|sh",
            "exécution de code téléchargé via un pipe vers un shell",
        ),
        (
            "| bash",
            "exécution de code téléchargé via un pipe vers un shell",
        ),
        (
            "|bash",
            "exécution de code téléchargé via un pipe vers un shell",
        ),
        ("sudo rm", "suppression en tant que superutilisateur"),
    ];
    patterns
        .iter()
        .find(|(p, _)| lower.contains(p))
        .map(|(_, why)| *why)
}

/// Commandes en **lecture seule / inspection** considérées comme sûres : elles
/// s'exécutent sans demander de confirmation. Toute commande qui modifie l'état
/// (build/test/run/install, écriture de fichiers, exécution de code du projet)
/// est volontairement exclue et doit être approuvée par l'utilisateur.
const SAFE_COMMAND_PREFIXES: &[&str] = &[
    // Inspection et lecture
    "echo",
    "ls",
    "dir",
    "pwd",
    "cat",
    "head",
    "tail",
    "wc",
    "tac",
    "nl",
    "rg",
    "grep",
    "egrep",
    "fgrep",
    "which",
    "where",
    "stat",
    "file",
    "tree",
    "du",
    "df",
    "env",
    "printenv",
    "date",
    "whoami",
    "uname",
    "hostname",
    "type",
    "printf",
    "sort",
    "uniq",
    "cut",
    "tr",
    "basename",
    "dirname",
    "realpath",
    "diff",
    "cmp",
    "sed -n",
    "awk",
    // Git en lecture seule
    "git status",
    "git diff",
    "git log",
    "git show",
    "git rev-parse",
    "git ls-files",
    "git describe",
    "git config --get",
    "git remote -v",
    "git branch --list",
    "git branch -a",
    // Versions / outils (sans exécution de code)
    "cargo --version",
    "cargo -V",
    "rustc --version",
    "node --version",
    "node -v",
    "python --version",
    "python3 --version",
    "npm --version",
    "npm -v",
    "go version",
    "java -version",
    "pip list",
    "pip freeze",
    "pip --version",
    "tsc --version",
    // Créations non destructrices
    "mkdir",
    "touch",
];

/// Indique si une commande peut s'exécuter **sans confirmation** de l'utilisateur.
///
/// La commande est décomposée sur les opérateurs shell (`&&`, `||`, `;`, `|`) :
/// chaque sous-commande doit commencer par un préfixe sûr, et la commande ne doit
/// contenir ni redirection, ni substitution, ni exécution dynamique, qui
/// permettraient de contourner le garde-fou.
pub fn is_pre_approved_command(cmd: &str) -> bool {
    if dangerous_command_reason(cmd).is_some() {
        return false;
    }
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Refuse redirections, substitution de commande et expansion de variables.
    if trimmed.contains('>')
        || trimmed.contains('<')
        || trimmed.contains('`')
        || trimmed.contains("$(")
        || trimmed.contains("${")
        || trimmed.contains('$')
    {
        return false;
    }

    for segment in trimmed.split(['&', '|', ';', '\n']) {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }
        let lower = seg.to_lowercase();
        let is_safe = SAFE_COMMAND_PREFIXES.iter().any(|p| {
            lower == *p
                || lower.starts_with(&format!("{} ", p))
                || lower.starts_with(&format!("{}\t", p))
        });
        if !is_safe {
            return false;
        }
    }
    true
}

/// Durée maximale d'une commande : courte pour ne jamais bloquer l'interface.
pub fn command_timeout(cmd: &str) -> Duration {
    let lower = cmd.to_lowercase();
    let slow = [
        "install",
        "build",
        "npm",
        "pip",
        "cargo",
        "npx",
        "startproject",
        "startapp",
        "migrate",
        "create",
    ];
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

/// Build the platform shell invocation for `cmd` (Git Bash on Windows,
/// `bash`/`sh` on Unix, with a `cmd` fallback on Windows).
fn build_shell_command(cmd: &str) -> tokio::process::Command {
    let clean_cmd = normalize_bash_command(cmd);
    if let Some(bash_bin) = crate::platform::find_bash_executable() {
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
    }
}

/// Apply an optional working directory that actually exists.
fn set_working_dir(command: &mut tokio::process::Command, cwd: Option<&str>) {
    if let Some(dir) = cwd {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            let path = Path::new(trimmed);
            if path.exists() {
                command.current_dir(path);
            }
        }
    }
}

/// One line of output, tagged with the stream it came from.
struct StreamLine {
    stderr: bool,
    text: String,
}

/// Spawn a task that forwards every line of `reader` to `tx`.
fn spawn_line_reader<R>(
    reader: Option<R>,
    stderr: bool,
    tx: tokio::sync::mpsc::Sender<StreamLine>,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let Some(reader) = reader else { return };
        let mut lines = tokio::io::BufReader::new(reader).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            if tx.send(StreamLine { stderr, text }).await.is_err() {
                break;
            }
        }
    })
}

/// Executes a shell command and emits each output line to `on_line` **as it is
/// produced**, so callers can display the execution live (terminal style).
///
/// stdout and stderr are captured concurrently so a chatty build never blocks
/// the other stream, and the whole run is bounded by `timeout`. Returns
/// `(success, formatted_output)` where `success` is `false` on a non-zero exit
/// status, a spawn error, or a timeout.
pub async fn run_shell_command_streaming<F>(
    cmd: &str,
    cwd: Option<&str>,
    timeout: Duration,
    mut on_line: F,
) -> (bool, String)
where
    F: FnMut(&str),
{
    let mut command = build_shell_command(cmd);
    set_working_dir(&mut command, cwd);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return (
                false,
                format!("Erreur lors de l'exécution de la commande bash: {}", e),
            );
        }
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamLine>(256);
    let stdout_task = spawn_line_reader(child.stdout.take(), false, tx.clone());
    let stderr_task = spawn_line_reader(child.stderr.take(), true, tx.clone());
    drop(tx);

    let mut stdout_acc = String::new();
    let mut stderr_acc = String::new();

    let drain = async {
        while let Some(line) = rx.recv().await {
            if line.stderr {
                stderr_acc.push_str(&line.text);
                stderr_acc.push('\n');
            } else {
                stdout_acc.push_str(&line.text);
                stdout_acc.push('\n');
            }
            on_line(&line.text);
        }
    };

    let timed_out = tokio::time::timeout(timeout, drain).await.is_err();
    if timed_out {
        let _ = child.kill().await;
    }
    let status = child.wait().await;
    // Drop the receiver before joining the reader tasks: if the buffer filled
    // up (verbose build) a reader may be blocked on `send`, which only unblocks
    // once the receiver is gone.
    drop(rx);
    if timed_out {
        // On timeout a reader may be stuck on a pipe that a background child
        // still holds open; abort instead of blocking the caller.
        stdout_task.abort();
        stderr_task.abort();
    } else {
        let _ = stdout_task.await;
        let _ = stderr_task.await;
    }

    if timed_out {
        return (
            false,
            format!(
                "Délai d'exécution dépassé (timeout {}s).",
                timeout.as_secs()
            ),
        );
    }

    match status {
        Ok(status) => {
            let ok = status.success();
            let truncated_stdout = truncate_output(&stdout_acc, 30, 2000);
            let truncated_stderr = truncate_output(&stderr_acc, 30, 2000);
            (
                ok,
                format!(
                    "Statut (bash): {}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
                    status, truncated_stdout, truncated_stderr
                ),
            )
        }
        Err(e) => (
            false,
            format!("Erreur lors de l'exécution de la commande bash: {}", e),
        ),
    }
}

/// Executes a shell command via `bash` (Git Bash on Windows, `bash`/`sh` on Unix)
/// and returns `(success, formatted_output)`.
///
/// `success` is `false` on a non-zero exit status, on a spawn error, or on
/// timeout. Callers that need to know whether the command actually succeeded
/// (e.g. the project verifier) should use this instead of [`run_shell_command`].
pub async fn run_shell_command_capture(
    cmd: &str,
    cwd: Option<&str>,
    timeout: Duration,
) -> (bool, String) {
    run_shell_command_streaming(cmd, cwd, timeout, |_| {}).await
}

/// Same as [`run_shell_command_capture`] but with an explicit timeout, returning
/// only the formatted output.
pub async fn run_shell_command_with_timeout(
    cmd: &str,
    cwd: Option<&str>,
    timeout: Duration,
) -> String {
    run_shell_command_capture(cmd, cwd, timeout).await.1
}

/// Executes a shell command via `bash` (Git Bash on Windows, `bash`/`sh` on Unix) with the
/// default per-command timeout.
pub async fn run_shell_command(cmd: &str, cwd: Option<&str>) -> String {
    let limit = command_timeout(cmd);
    run_shell_command_with_timeout(cmd, cwd, limit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The streaming variant must forward the command's output line by line.
    #[tokio::test]
    async fn streaming_emits_output_lines() {
        let mut lines: Vec<String> = Vec::new();
        let (ok, out) = run_shell_command_streaming(
            "echo rustagent-stream",
            None,
            Duration::from_secs(15),
            |line| lines.push(line.to_string()),
        )
        .await;

        assert!(ok, "command failed: {}", out);
        assert!(
            lines.iter().any(|l| l.contains("rustagent-stream")),
            "streamed lines: {:?}",
            lines
        );
        assert!(out.contains("Statut (bash)"), "formatted output: {}", out);
    }

    /// A command that exceeds the timeout is reported as a failure (the exact
    /// message depends on the shell, so only the failure is asserted).
    #[tokio::test]
    async fn streaming_times_out() {
        let (ok, _out) =
            run_shell_command_streaming("sleep 5", None, Duration::from_secs(1), |_| {}).await;
        assert!(
            !ok,
            "a command exceeding the timeout must be reported as failed"
        );
    }
}
