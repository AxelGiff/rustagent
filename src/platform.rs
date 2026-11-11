//! Utilitaires multi-plateformes pour le terminal intégré et l'exécution de code.
//!
//! Tout le comportement spécifique à une plateforme (sélection du shell, variables
//! d'environnement, noms des exécutables et commandes d'ouverture de fichiers) est
//! centralisé ici afin que le reste de l'application puisse rester agnostique à la plateforme.
//!
//! # Plateformes supportées
//! - Windows (via `cfg!(windows)`) — privilégie Git Bash (`bash.exe`)
//! - macOS (via `cfg!(target_os = "macos")`)
//! - Linux / autres Unix (comportement par défaut)

/// Recherche l'exécutable `bash` (Git Bash sur Windows, ou `bash` standard sur Unix).
pub fn find_bash_executable() -> Option<String> {
    if cfg!(windows) {
        let mut candidates = vec![
            std::path::PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
            std::path::PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"),
            std::path::PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"),
        ];
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            candidates
                .push(std::path::PathBuf::from(local_app_data).join(r"Programs\Git\bin\bash.exe"));
        }
        if let Some(userprofile) = std::env::var_os("USERPROFILE") {
            candidates.push(std::path::PathBuf::from(userprofile).join(r"scoop\shims\bash.exe"));
        }
        for candidate in candidates {
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
        // Vérifier dans le PATH
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let b = dir.join("bash.exe");
                // Éviter le stub WSL System32\bash.exe si aucun distro WSL n'est souhaité, privilégier Git/MSYS
                let lower = b.to_string_lossy().to_lowercase();
                if b.is_file() && !lower.contains("system32") {
                    return Some(b.to_string_lossy().to_string());
                }
            }
        }
        None
    } else {
        Some("bash".to_string())
    }
}

/// Le shell utilisé pour lancer le terminal intégré.
///
/// Privilégie `bash` (Git Bash sur Windows, `bash` sur Linux/macOS) afin d'éviter
/// les erreurs de syntaxe PowerShell/cmd.exe.
pub fn terminal_shell() -> String {
    if let Some(bash) = find_bash_executable() {
        bash
    } else if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        "bash".to_string()
    }
}

/// Arguments de ligne de commande supplémentaires passés au shell du terminal afin
/// qu'il signale son répertoire de travail courant via OSC 7 à chaque invite.
pub fn terminal_shell_args() -> Vec<String> {
    let uses_bash = find_bash_executable().is_some() || !cfg!(windows);
    if uses_bash {
        vec![
            "--init-file".to_string(),
            osc7_init_file(true).display().to_string(),
        ]
    } else {
        vec![
            "-NoExit".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            osc7_init_file(false).display().to_string(),
        ]
    }
}

/// Chemin vers un fichier d'initialisation de shell généré qui fait que le shell
/// signale son répertoire de travail courant via OSC 7 à chaque invite.
fn osc7_init_file(for_bash: bool) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("rustagent");
    let _ = std::fs::create_dir_all(&dir);

    if !for_bash && cfg!(windows) {
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
        let content = if cfg!(windows) {
            r#"# rustagent fichier d'initialisation OSC 7 pour Git Bash (Windows).
__rustagent_start_pwd="$(pwd)"
export CHERE_INVOKING=1
if [ -f /etc/profile ]; then
    . /etc/profile
fi
if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi
cd "$__rustagent_start_pwd" 2>/dev/null || true
unset __rustagent_start_pwd

__rustagent_osc7() {
    local win_pwd
    win_pwd="$(pwd -W 2>/dev/null || pwd)"
    win_pwd="${win_pwd//$'\r'/}"
    win_pwd="${win_pwd//\\//}"
    win_pwd="${win_pwd// /%20}"
    printf '\033]7;file:///%s\007' "$win_pwd"
}
PROMPT_COMMAND="__rustagent_osc7"
"#
        } else {
            r#"# rustagent fichier d'initialisation OSC 7.
if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

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
"#
        };
        // Toujours écrire osc7.bash avec des fins de ligne Unix (\n) même si ce fichier source est en CRLF
        let normalized = content.replace("\r\n", "\n");
        let _ = std::fs::write(&path, normalized);
        path
    }
}

/// Variables d'environnement à définir sur le shell du terminal.
pub fn terminal_env() -> Vec<(&'static str, &'static str)> {
    let mut env = Vec::new();
    if find_bash_executable().is_some() || !cfg!(windows) {
        env.push(("TERM", "xterm-256color"));
        env.push(("COLORTERM", "truecolor"));
        // Empêche `/etc/profile` de Git Bash (MSYS2) de faire `cd "$HOME"` au démarrage
        env.push(("CHERE_INVOKING", "1"));
    }
    if !cfg!(windows) {
        env.push(("LANG", "en_GB.UTF-8"));
    }
    env
}

/// La commande de l'interpréteur Python.
///
/// - Windows : `python` (le lanceur standard).
/// - Unix : `python3` (le binaire version 3 standard sur la plupart des distributions).
///
/// # Retour
/// Le nom de la commande à exécuter pour lancer Python.
pub fn python_command() -> &'static str {
    // Sur Windows, le lanceur `python` est le standard ; sur Unix,
    // `python3` est préféré car `python` pointe souvent vers Python 2.
    if cfg!(windows) { "python" } else { "python3" }
}

/// Construit la commande qui ouvre un fichier avec l'application par défaut.
///
/// - Windows : `start "" "file"` (utilise l'association de programmes Windows).
/// - macOS : `open "file"` (utilise LaunchServices).
/// - Linux : `xdg-open "file"` (respecte les associations MIME du bureau).
///
/// # Paramètres
/// - `file` : chemin du fichier à ouvrir.
///
/// # Retour
/// La chaîne de commande complète, incluant le retour chariot (Windows) ou
/// saut de ligne (Unix) pour exécuter la commande dans le terminal.
pub fn open_command(file: &str) -> String {
    if cfg!(windows) {
        // Windows : `start` est une commande interne de cmd.exe.
        // La chaîne vide en premier argument évite d'interpréter le chemin comme un titre.
        format!("start \"\" \"{}\"\r\n", file)
    } else if cfg!(target_os = "macos") {
        // macOS : la commande `open` délègue au système d'ouverture native.
        format!("open \"{}\"\n", file)
    } else {
        // Linux et autres Unix : `xdg-open` respecte les associations de fichiers du système.
        format!("xdg-open \"{}\"\n", file)
    }
}

/// La commande du compilateur C.
///
/// - Windows : `gcc` (suppose MinGW ou similaire dans le PATH).
/// - Unix : `gcc`.
pub fn c_compiler() -> &'static str {
    // GCC est utilisé partout ; aucune variante par plateforme n'est nécessaire.
    "gcc"
}

/// La commande du compilateur C++.
///
/// - Windows : `g++` (suppose MinGW ou similaire dans le PATH).
/// - Unix : `g++`.
pub fn cpp_compiler() -> &'static str {
    // G++ est le pendant C++ de GCC, disponible sur toutes les plateformes.
    "g++"
}

/// La commande du compilateur Java.
///
/// `javac` est le compilateur officiel du JDK, présent sur toutes les plateformes
/// où le JDK est installé.
pub fn java_compiler() -> &'static str {
    "javac"
}

/// La commande d'exécution Java (runtime).
///
/// `java` lance la JVM pour exécuter des fichiers `.class` compilés.
pub fn java_runtime() -> &'static str {
    "java"
}

/// La commande d'exécution Go.
///
/// La sous-commande `go run` est utilisée pour exécuter directement les fichiers `.go`.
pub fn go_runner() -> &'static str {
    "go"
}

/// La commande d'exécution Node.js.
///
/// `node` exécute les fichiers JavaScript directement.
pub fn node_runner() -> &'static str {
    "node"
}

/// La commande d'exécution TypeScript (via ts-node).
///
/// Utilise `npx` pour exécuter `ts-node`, qui compile et exécute le TypeScript
/// à la volée sans étape de compilation séparée.
pub fn ts_runner() -> &'static str {
    "npx ts-node"
}

/// La commande du compilateur Rust.
///
/// `rustc` est le compilateur officiel du langage Rust.
pub fn rust_compiler() -> &'static str {
    "rustc"
}

/// Retourne le répertoire de configuration de l'application selon la plateforme.
///
/// - **Windows** : `%APPDATA%\rustagent`
/// - **macOS** : `~/Library/Application Support/rustagent`
/// - **Linux** : `$XDG_CONFIG_HOME/rustagent` ou `~/.config/rustagent`
///
/// Si aucune variable d'environnement standard n'est disponible, un répertoire
/// local `.rustagent` dans le répertoire courant est utilisé en repli.
pub fn config_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        // Windows : la variable APPDATA (Roaming) est le standard pour les configs.
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return std::path::PathBuf::from(appdata).join("rustagent");
        }
    } else if cfg!(target_os = "macos") {
        // macOS : ~/Library/Application Support est l'emplacement conventionnel.
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("rustagent");
        }
    } else {
        // Linux : la spécification XDG Base Directory est prioritaire.
        // 1. Si XDG_CONFIG_HOME est défini, on l'utilise.
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return std::path::PathBuf::from(xdg).join("rustagent");
        }
        // 2. Sinon, on retombe sur ~/.config, comme le veut la spécification.
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home)
                .join(".config")
                .join("rustagent");
        }
    }
    // Repli final : un répertoire local au projet courant.
    std::path::PathBuf::from(".rustagent")
}

/// Ouvre une boîte de dialogue native du système d'exploitation pour sélectionner un dossier de projet.
///
/// Cette fonction est bloquante le temps que l'utilisateur choisisse le dossier : elle doit être
/// appelée depuis `tokio::task::spawn_blocking`.
pub fn pick_folder_dialog(initial_dir: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        let init_str = initial_dir
            .map(|p| p.to_string_lossy().replace('\'', "''"))
            .unwrap_or_default();
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $dlg = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $dlg.Description = 'Sélectionner le dossier du projet à ouvrir dans RustAgent'; \
             $dlg.ShowNewFolderButton = $true; \
             if ('{}' -ne '' -and (Test-Path '{}')) {{ $dlg.SelectedPath = '{}' }}; \
             if ($dlg.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ \
                 [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
                 Write-Output $dlg.SelectedPath \
             }}",
            init_str, init_str, init_str
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Sta", "-WindowStyle", "Hidden", "-Command", &script])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stdout.is_empty() {
            let path = std::path::PathBuf::from(stdout);
            if path.is_dir() {
                return Some(path);
            }
        }
        None
    } else if cfg!(target_os = "macos") {
        let output = std::process::Command::new("osascript")
            .args([
                "-e",
                "POSIX path of (choose folder with prompt \"Sélectionner le dossier du projet\")",
            ])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stdout.is_empty() {
            let path = std::path::PathBuf::from(stdout);
            if path.is_dir() {
                return Some(path);
            }
        }
        None
    } else {
        let output = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--directory",
                "--title=Sélectionner le dossier du projet",
            ])
            .output()
            .or_else(|_| {
                std::process::Command::new("kdialog")
                    .args(["--getexistingdirectory", "."])
                    .output()
            })
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stdout.is_empty() {
            let path = std::path::PathBuf::from(stdout);
            if path.is_dir() {
                return Some(path);
            }
        }
        None
    }
}

/// Construit la commande shell permettant au terminal intégré de se déplacer dans `dir`.
pub fn cd_command(dir: &std::path::Path) -> String {
    let uses_bash = find_bash_executable().is_some() || !cfg!(windows);
    let normalized = dir.to_string_lossy().replace('\\', "/").replace('"', "\\\"");
    if uses_bash {
        format!("cd \"{}\"\n", normalized)
    } else {
        format!("Set-Location \"{}\"\r\n", normalized)
    }
}

/// Recherche le binaire ou script CLI de VS Code (`code.cmd`, `code.exe`, ou `code`).
pub fn find_vscode_command() -> Option<String> {
    if cfg!(windows) {
        let mut candidates = Vec::new();
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let p = std::path::PathBuf::from(&local_app_data)
                .join(r"Programs\Microsoft VS Code\bin\code.cmd");
            candidates.push(p);
            let p_exe = std::path::PathBuf::from(&local_app_data)
                .join(r"Programs\Microsoft VS Code\Code.exe");
            candidates.push(p_exe);
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                std::path::PathBuf::from(program_files)
                    .join(r"Microsoft VS Code\bin\code.cmd"),
            );
        }
        for c in candidates {
            if c.is_file() {
                return Some(c.to_string_lossy().to_string());
            }
        }
        // Tentative via PATH standard
        Some("code.cmd".to_string())
    } else {
        Some("code".to_string())
    }
}

/// Ouvre un fichier (ou dossier) dans VS Code, éventuellement à une ligne précise (`code -g path:line`).
pub fn open_in_vscode(path: &std::path::Path, line: Option<usize>) -> bool {
    let Some(cmd) = find_vscode_command() else {
        return false;
    };
    let mut command = std::process::Command::new(&cmd);
    if let Some(l) = line {
        command.arg("-g").arg(format!("{}:{}", path.display(), l));
    } else {
        command.arg(path);
    }
    command.spawn().is_ok()
}

/// Ouvre le comparateur de Diff natif de VS Code (`code --diff <ancien> <nouveau>`).
/// Le contenu proposé par l'agent est écrit dans un fichier temporaire en lecture seule.
pub fn diff_in_vscode(disk_path: &std::path::Path, proposed_content: &str) -> bool {
    let Some(cmd) = find_vscode_command() else {
        return false;
    };
    let file_name = disk_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let temp_diff_file = std::env::temp_dir().join(format!("rustagent_diff_{}", file_name));
    if std::fs::write(&temp_diff_file, proposed_content).is_err() {
        return false;
    }
    std::process::Command::new(&cmd)
        .args(["--diff"])
        .arg(disk_path)
        .arg(&temp_diff_file)
        .spawn()
        .is_ok()
}
