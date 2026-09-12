//! Utilitaires multi-plateformes pour le terminal intégré et l'exécution de code.
//!
//! Tout le comportement spécifique à une plateforme (sélection du shell, variables
//! d'environnement, noms des exécutables et commandes d'ouverture de fichiers) est
//! centralisé ici afin que le reste de l'application puisse rester agnostique à la plateforme.

/// Le shell utilisé pour lancer le terminal intégré.
///
/// - Windows : PowerShell (largement disponible et scriptable).
/// - Unix (Linux/macOS) : bash.
pub fn terminal_shell() -> &'static str {
    if cfg!(windows) {
        "powershell.exe"
    } else {
        "bash"
    }
}

/// Arguments de ligne de commande supplémentaires passés au shell du terminal
/// afin qu'il signale son répertoire de travail courant via OSC 7 à chaque invite.
///
/// La barre latérale de l'arborescence de fichiers s'appuie sur le répertoire de
/// travail signalé par le terminal pour savoir quel dossier afficher, le shell doit
/// donc émettre activement la séquence OSC 7 (bash ne le fait pas par défaut).
///
/// - Windows : PowerShell est configuré pour émettre OSC 7 depuis sa fonction `prompt`.
/// - Unix : bash est lancé avec un fichier d'initialisation qui configure l'émission OSC 7.
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

/// Chemin vers un fichier d'initialisation de shell généré qui fait que le shell
/// signale son répertoire de travail courant via OSC 7 à chaque invite.
///
/// Le fichier est écrit dans le répertoire temporaire de la plateforme et est
/// idempotent : il est régénéré à chaque appel, reflétant ainsi toujours la logique actuelle.
fn osc7_init_file() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("rustagent");
    let _ = std::fs::create_dir_all(&dir);

    if cfg!(windows) {
        let path = dir.join("osc7.ps1");
        let content = r#"# rustagent fichier d'initialisation OSC 7 pour PowerShell.
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
        let content = r#"# rustagent fichier d'initialisation OSC 7.
# Préserve les personnalisations bash normales de l'utilisateur.
if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

# Émet le répertoire de travail courant via OSC 7 afin que l'application puisse le suivre.
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

/// Variables d'environnement à définir sur le shell du terminal.
///
/// `TERM`, `COLORTERM` et `LANG` sont spécifiques à Unix et ne sont définies que
/// sur les plateformes non-Windows. Les terminaux Windows n'utilisent pas ces variables.
pub fn terminal_env() -> Vec<(&'static str, &'static str)> {
    let mut env = Vec::new();
    if !cfg!(windows) {
        env.push(("TERM", "xterm-256color"));
        env.push(("COLORTERM", "truecolor"));
        env.push(("LANG", "en_GB.UTF-8"));
    }
    env
}

/// La commande de l'interpréteur Python.
///
/// - Windows : `python` (le lanceur standard).
/// - Unix : `python3`.
pub fn python_command() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// Construit la commande qui ouvre un fichier avec l'application par défaut.
///
/// - Windows : `start "" "file"`.
/// - macOS : `open "file"`.
/// - Linux : `xdg-open "file"`.
pub fn open_command(file: &str) -> String {
    if cfg!(windows) {
        format!("start \"\" \"{}\"\r\n", file)
    } else if cfg!(target_os = "macos") {
        format!("open \"{}\"\n", file)
    } else {
        format!("xdg-open \"{}\"\n", file)
    }
}

/// La commande du compilateur C.
///
/// - Windows : `gcc` (suppose MinGW ou similaire dans le PATH).
/// - Unix : `gcc`.
pub fn c_compiler() -> &'static str {
    "gcc"
}

/// La commande du compilateur C++.
///
/// - Windows : `g++` (suppose MinGW ou similaire dans le PATH).
/// - Unix : `g++`.
pub fn cpp_compiler() -> &'static str {
    "g++"
}

/// La commande du compilateur Java.
pub fn java_compiler() -> &'static str {
    "javac"
}

/// La commande d'exécution Java (runtime).
pub fn java_runtime() -> &'static str {
    "java"
}

/// La commande d'exécution Go.
pub fn go_runner() -> &'static str {
    "go"
}

/// La commande d'exécution Node.js.
pub fn node_runner() -> &'static str {
    "node"
}

/// La commande d'exécution TypeScript (via ts-node).
pub fn ts_runner() -> &'static str {
    "npx ts-node"
}

/// La commande du compilateur Rust.
pub fn rust_compiler() -> &'static str {
    "rustc"
}

/// Le répertoire où l'application stocke ses fichiers de configuration.
///
/// - Windows : `%APPDATA%\rustagent`
/// - macOS : `$HOME/Library/Application Support/rustagent`
/// - Linux : `$XDG_CONFIG_HOME/rustagent` ou `$HOME/.config/rustagent`
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
        // Linux / autres Unix
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return std::path::PathBuf::from(xdg).join("rustagent");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home)
                .join(".config")
                .join("rustagent");
        }
    }
    // Repli : répertoire courant
    std::path::PathBuf::from(".rustagent")
}
