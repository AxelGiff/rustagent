//! Utilitaires multi-plateformes pour le terminal intégré et l'exécution de code.
//!
//! Tout le comportement spécifique à une plateforme (sélection du shell, variables
//! d'environnement, noms des exécutables et commandes d'ouverture de fichiers) est
//! centralisé ici afin que le reste de l'application puisse rester agnostique à la plateforme.
//!
//! # Plateformes supportées
//! - Windows (via `cfg!(windows)`)
//! - macOS (via `cfg!(target_os = "macos")`)
//! - Linux / autres Unix (comportement par défaut)
//!
//! Les fonctions de ce module utilisent des macros de compilation conditionnelles
//! (`cfg!`) pour sélectionner le comportement approprié à chaque plateforme.

/// Le shell utilisé pour lancer le terminal intégré.
///
/// - Windows : PowerShell (largement disponible et scriptable).
/// - Unix (Linux/macOS) : bash.
///
/// # Retour
/// Le nom de l'exécutable du shell sous forme de chaîne statique.
pub fn terminal_shell() -> &'static str {
    // Détection de la plateforme au moment de la compilation :
    // la branche Windows est compilée uniquement sur Windows,
    // la branche Unix est compilée sur toutes les autres plateformes.
    if cfg!(windows) {
        "powershell.exe"
    } else {
        "bash"
    }
}

/// Arguments de ligne de commande supplémentaires passés au shell du terminal afin
/// qu'il signale son répertoire de travail courant via OSC 7 à chaque invite.
///
/// La barre latérale de l'arborescence de fichiers s'appuie sur le répertoire de
/// travail signalé par le terminal pour savoir quel dossier afficher. Le shell doit
/// donc émettre activement la séquence OSC 7 (bash ne le fait pas par défaut).
///
/// Ces arguments diffèrent selon la plateforme :
/// - **Windows** : `-NoExit -ExecutionPolicy Bypass -File <osc7.ps1>`
///   → PowerShell reste ouvert, contourne la politique d'exécution et charge
///   le script OSC 7.
/// - **Unix** : `--init-file <osc7.bash>`
///   → bash charge le fichier d'initialisation OSC 7 au démarrage.
///
/// Les arguments vides sont filtrés afin de ne conserver que les paramètres
/// réellement nécessaires pour la plateforme courante.
pub fn terminal_shell_args() -> Vec<String> {
    // Construction sélective des arguments selon la plateforme.
    // Chaque case correspond à une position précise dans la ligne de commande.
    vec![
        // Position 1 : paramètre principal du shell
        if cfg!(windows) {
            "-NoExit".to_string()          // PowerShell : reste ouvert après exécution
        } else {
            "--init-file".to_string()      // bash : charge un fichier d'init personnalisé
        },
        // Position 2 : valeur du paramètre précédent
        if cfg!(windows) {
            "-ExecutionPolicy".to_string() // PowerShell : politique d'exécution
        } else {
            osc7_init_file().display().to_string() // bash : chemin du fichier OSC 7
        },
        // Position 3 : valeur de l'ExecutionPolicy (Windows uniquement)
        if cfg!(windows) {
            "Bypass".to_string()           // Contourne la politique de sécurité
        } else {
            "".to_string()                 // Non utilisé sur Unix
        },
        // Position 4 : flag -File pour PowerShell
        if cfg!(windows) {
            "-File".to_string()            // Indique qu'on passe un fichier script
        } else {
            "".to_string()                 // Non utilisé sur Unix
        },
        // Position 5 : chemin du script OSC 7 (Windows uniquement)
        if cfg!(windows) {
            osc7_init_file().display().to_string() // Script PowerShell OSC 7
        } else {
            "".to_string()                 // Non utilisé sur Unix
        },
    ]
    .into_iter()
    // Filtre les chaînes vides pour ne garder que les arguments pertinents
    .filter(|s| !s.is_empty())
    .collect()
}

/// Chemin vers un fichier d'initialisation de shell généré qui fait que le shell
/// signale son répertoire de travail courant via OSC 7 à chaque invite.
///
/// Le fichier est écrit dans le répertoire temporaire de la plateforme et est
/// idempotent : il est régénéré à chaque appel, reflétant ainsi toujours la logique actuelle.
///
/// # OSC 7
/// OSC 7 est une séquence d'échappement de terminal (\x1b]7;...) qui permet
/// au shell de communiquer son répertoire de travail à l'application hôte.
/// C'est utilisé par l'arborescence de fichiers intégrée pour suivre le dossier courant.
///
/// # Retour
/// Le chemin complet vers le fichier d'initialisation généré (`osc7.ps1` sur Windows,
/// `osc7.bash` sur Unix).
fn osc7_init_file() -> std::path::PathBuf {
    // Répertoire temporaire dédié à rustagent : <temp>/rustagent/
    let dir = std::env::temp_dir().join("rustagent");
    // Crée le répertoire s'il n'existe pas (échec silencieux s'il existe déjà)
    let _ = std::fs::create_dir_all(&dir);

    if cfg!(windows) {
        // --- Windows : script PowerShell ---
        let path = dir.join("osc7.ps1");
        // Le script redéfinit la fonction `prompt` de PowerShell pour qu'elle
        // émette la séquence OSC 7 avec le chemin PWD (encodé en URL).
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
        // Écrit le fichier (échec silencieux en cas d'erreur)
        let _ = std::fs::write(&path, content);
        path
    } else {
        // --- Unix (Linux/macOS) : script bash ---
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
        // Écrit le fichier (échec silencieux en cas d'erreur)
        let _ = std::fs::write(&path, content);
        path
    }
}

/// Variables d'environnement à définir sur le shell du terminal.
///
/// `TERM`, `COLORTERM` et `LANG` sont spécifiques à Unix et ne sont définies que
/// sur les plateformes non-Windows. Les terminaux Windows n'utilisent pas ces variables.
///
/// # Retour
/// Une liste de paires (clé, valeur) à injecter dans l'environnement du shell.
pub fn terminal_env() -> Vec<(&'static str, &'static str)> {
    let mut env = Vec::new();
    // Ces variables sont actuellement uniquement utilisées par les terminaux Unix.
    if !cfg!(windows) {
        env.push(("TERM", "xterm-256color")); // Type de terminal : couleurs étendues xterm
        env.push(("COLORTERM", "truecolor")); // Support des couleurs 24 bits (truecolor)
        env.push(("LANG", "en_GB.UTF-8"));    // Locale : anglais britannique UTF-8
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
