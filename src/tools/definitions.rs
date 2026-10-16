//! OpenAI-compatible tool definitions and schemas for native RustAgent tools.

use serde_json::{json, Value};
use crate::diff::PendingDiff;

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
                "description": "Crée ou remplace un ou plusieurs fichiers avec leur contenu complet. Pour créer plusieurs fichiers d'un projet complet en un seul appel, utilise le paramètre `files`. Pour un seul fichier, utilise `path` et `content`. Génère un diff prêt à valider par l'utilisateur.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin du fichier à créer ou modifier (ex: 'Pictures/halloween-menu/index.html')."
                        },
                        "content": {
                            "type": "string",
                            "description": "Contenu textuel complet du code du fichier."
                        },
                        "files": {
                            "type": "array",
                            "description": "Liste de tous les fichiers du projet à générer d'un coup (ex: index.html, style.css, script.js).",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string", "description": "Chemin du fichier (ex: 'Pictures/halloween-menu/style.css')" },
                                    "content": { "type": "string", "description": "Code complet du fichier" }
                                },
                                "required": ["path", "content"]
                            }
                        }
                    }
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
                "description": "Liste uniquement les fichiers pertinents d'un sous-dossier précis (avec filtre optionnel `filter`). RÈGLE STRICTE : N'appelez JAMAIS `list_directory` pour lister tous les fichiers du projet ! Ciblez uniquement le dossier directement lié à la fonctionnalité prévue et utilisez `filter` pour ne lister que les fichiers nécessaires.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin précis du sous-dossier de la fonctionnalité (ex: 'src', './mon_projet/src/auth')."
                        },
                        "filter": {
                            "type": "string",
                            "description": "Filtre optionnel par mot-clé ou extension (ex: 'auth', '.rs', 'user') pour ne retourner que les fichiers utiles à la fonctionnalité prévue."
                        }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Recherche une chaîne de caractères ou motif dans les fichiers du projet (à privilégier plutôt que de lister tout le projet).",
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
                "description": "Exécute une commande dans un terminal **bash** (Git Bash / Unix bash). IMPORTANT : Écrivez EXCLUSIVEMENT des commandes **bash** standard (ex: 'mkdir -p ... && ...', chemins avec '/', 'rm -f', 'cargo test', 'npm install'). N'utilisez JAMAIS de syntaxe PowerShell ni cmd.exe.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Commande bash (syntaxe Unix/POSIX standard) à exécuter."
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
        json!({
            "type": "function",
            "function": {
                "name": "ask_spec_questions",
                "description": "Pose 1 à 3 questions rapides à choix multiples à l'utilisateur pour clarifier les spécifications et les choix Security by Design avant de coder. Chaque question affichera automatiquement un champ 'Autre' dans l'interface.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "questions": {
                            "type": "array",
                            "description": "Liste de 1 à 3 questions courtes.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string", "description": "Identifiant court de la question (ex: 'auth', 'db')." },
                                    "question": { "type": "string", "description": "La question concise à poser à l'utilisateur." },
                                    "options": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "2 à 4 options claires (mettre l'option recommandée Security by Design en premier). Ne pas inclure 'Autre', l'UI l'ajoute automatiquement."
                                    }
                                },
                                "required": ["question", "options"]
                            }
                        }
                    },
                    "required": ["questions"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "write_plan",
                "description": "Génère le fichier 'plan.md' contenant l'architecture, les spécifications et les mesures Security by Design du projet, et l'ajoute automatiquement au .gitignore du projet s'il existe.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin du fichier plan.md (ex: 'plan.md' ou './mon_projet/plan.md')."
                        },
                        "content": {
                            "type": "string",
                            "description": "Contenu Markdown complet du plan d'architecture et Security by Design."
                        }
                    },
                    "required": ["path", "content"]
                }
            }
        }),
    ]
}

