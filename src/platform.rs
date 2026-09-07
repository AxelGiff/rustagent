//! Cross-platform helpers for the integrated terminal and code execution.
//!
//! All platform-specific behavior (shell selection, environment variables,
//! executable names, and file-opening commands) is centralized here so that
//! the rest of the application can stay platform-agnostic.

/// The shell used to spawn the integrated terminal.
///
/// - Windows: PowerShell (widely available and scriptable).
/// - Unix (Linux/macOS): bash.
pub fn terminal_shell() -> &'static str {
    if cfg!(windows) {
        "powershell.exe"
    } else {
        "bash"
    }
}

/// Extra command-line arguments passed to the terminal shell so that it
/// reports its current working directory via OSC 7 on every prompt.
///
/// The file-tree sidebar relies on the terminal's reported working directory
/// to know which folder to display, so the shell must actively emit the OSC 7
/// sequence (bash does not do this by default).
///
/// - Windows: PowerShell is told to emit OSC 7 from its `prompt` function.
/// - Unix: bash is started with an init file that sets up OSC 7 emission.
pub fn terminal_shell_args() -> Vec<String> {
    vec![
        if cfg!(windows) {
            "-NoExit".to_string()
        } else {
            "--init-file".to_string()
        },
        if cfg!(windows) {
            "-ExecutionPolicy".to_string()
        } else {
            osc7_init_file().display().to_string()
        },
        if cfg!(windows) {
            "Bypass".to_string()
        } else {
            "".to_string()
        },
        if cfg!(windows) {
            "-File".to_string()
        } else {
            "".to_string()
        },
        if cfg!(windows) {
            osc7_init_file().display().to_string()
        } else {
            "".to_string()
        },
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect()
}

/// Path to a generated shell init file that makes the shell report its current
/// working directory via OSC 7 on every prompt.
///
/// The file is written to the platform temp directory and is idempotent: it is
/// regenerated on every call, so it always reflects the current logic.
fn osc7_init_file() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("rustagent");
    let _ = std::fs::create_dir_all(&dir);

    if cfg!(windows) {
        let path = dir.join("osc7.ps1");
        let content = r#"# rustagent OSC 7 init file for PowerShell.
function prompt {
    $p = $PWD.Path.Replace('\', '/').Replace(' ', '%20')
    $esc = [char]27
    $bel = [char]7
    $host.UI.Write("$esc]7;file:///$p$bel")
    "PS $PWD> "
}
Clear-Host
"#;
        let _ = std::fs::write(&path, content);
        path
    } else {
        let path = dir.join("osc7.bash");
        let content = r#"# rustagent OSC 7 init file.
# Preserve the user's normal bash customizations.
if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

# Emit the current working directory via OSC 7 so the app can track it.
__rustagent_osc7() {
    local encoded="" c
    local i
    for ((i = 0; i < ${#PWD}; i++)); do
        c="${PWD:i:1}"
        case "$c" in
            [a-zA-Z0-9/._~-]) encoded+="$c" ;;
            *) printf -v c '%%%02X' "'$c"; encoded+="$c" ;;
        esac
    done
    printf '\033]7;file://%s%s\007' "${HOSTNAME:-localhost}" "$encoded"
}
PROMPT_COMMAND="__rustagent_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
"#;
        let _ = std::fs::write(&path, content);
        path
    }
}

/// Environment variables to set on the terminal shell.
///
/// `TERM`, `COLORTERM`, and `LANG` are Unix-specific and are only set on
/// non-Windows platforms. Windows terminals do not use these variables.
pub fn terminal_env() -> Vec<(&'static str, &'static str)> {
    let mut env = Vec::new();
    if !cfg!(windows) {
        env.push(("TERM", "xterm-256color"));
        env.push(("COLORTERM", "truecolor"));
        env.push(("LANG", "en_GB.UTF-8"));
    }
    env
}

/// The Python interpreter command.
///
/// - Windows: `python` (the standard launcher).
/// - Unix: `python3`.
pub fn python_command() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// Build the command that opens a file with the default application.
///
/// - Windows: `start "" "file"`.
/// - macOS: `open "file"`.
/// - Linux: `xdg-open "file"`.
pub fn open_command(file: &str) -> String {
    if cfg!(windows) {
        format!("start \"\" \"{}\"\r\n", file)
    } else if cfg!(target_os = "macos") {
        format!("open \"{}\"\n", file)
    } else {
        format!("xdg-open \"{}\"\n", file)
    }
}

/// The C compiler command.
///
/// - Windows: `gcc` (assumes MinGW or similar in PATH).
/// - Unix: `gcc`.
pub fn c_compiler() -> &'static str {
    "gcc"
}

/// The C++ compiler command.
///
/// - Windows: `g++` (assumes MinGW or similar in PATH).
/// - Unix: `g++`.
pub fn cpp_compiler() -> &'static str {
    "g++"
}

/// The Java compiler command.
pub fn java_compiler() -> &'static str {
    "javac"
}

/// The Java runtime command.
pub fn java_runtime() -> &'static str {
    "java"
}

/// The Go runner command.
pub fn go_runner() -> &'static str {
    "go"
}

/// The Node.js runner command.
pub fn node_runner() -> &'static str {
    "node"
}

/// The TypeScript runner command (via ts-node).
pub fn ts_runner() -> &'static str {
    "npx ts-node"
}

/// The Rust compiler command.
pub fn rust_compiler() -> &'static str {
    "rustc"
}

/// The directory where the application stores its configuration files.
///
/// - Windows: `%APPDATA%\rustagent`
/// - macOS: `$HOME/Library/Application Support/rustagent`
/// - Linux: `$XDG_CONFIG_HOME/rustagent` or `$HOME/.config/rustagent`
pub fn config_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return std::path::PathBuf::from(appdata).join("rustagent");
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("rustagent");
        }
    } else {
        // Linux / other Unix
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return std::path::PathBuf::from(xdg).join("rustagent");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home)
                .join(".config")
                .join("rustagent");
        }
    }
    // Fallback: current directory
    std::path::PathBuf::from(".rustagent")
}
