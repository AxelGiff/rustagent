//! OpenAI-compatible tool definitions and schemas for native RustAgent tools.

use crate::diff::PendingDiff;
use serde_json::{Value, json};

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
                "description": "Lit le contenu d'un fichier du projet (y compris le contenu généré dans le tour en cours, pas encore écrit sur le disque). Utilise `start_line`/`end_line` pour cibler une plage. À appeler AVANT tout `edit_file` sur du code existant.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin relatif ou absolu du fichier à lire (ex: 'src/main.rs', 'Cargo.toml')."
                        },
                        "start_line": {
                            "type": "integer",
                            "description": "Première ligne à lire (1-based, optionnel)."
                        },
                        "end_line": {
                            "type": "integer",
                            "description": "Dernière ligne à lire, incluse (optionnel)."
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
                "description": "Modifie une section ciblée d'un fichier existant : remplace `target` par `replacement`. `target` doit être un extrait EXACT copié depuis `read_file` (plusieurs lignes si besoin). La comparaison tolère les différences d'indentation/espaces ; en cas d'échec, l'erreur renvoie le contenu du fichier pour corriger `target`. Génère un diff à valider. À privilégier sur `write_file` pour une modification ciblée.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Chemin du fichier à modifier."
                        },
                        "target": {
                            "type": "string",
                            "description": "Extrait EXACT existant à remplacer (copié depuis `read_file`)."
                        },
                        "replacement": {
                            "type": "string",
                            "description": "Nouveau texte de remplacement."
                        },
                        "replace_all": {
                            "type": "boolean",
                            "description": "Remplacer toutes les occurrences de `target` (défaut : false = première occurrence uniquement)."
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
                "name": "verify_project",
                "description": "Compile et teste le projet sur une COPIE TEMPORAIRE (le projet réel n'est pas modifié). Détecte automatiquement le type de projet (Cargo.toml → cargo check, package.json → npm install + build, tsconfig.json → tsc --noEmit, fichiers .js → node --check, .py → py_compile, go.mod → go build) et renvoie les erreurs. À appeler OBLIGATOIREMENT après avoir écrit les fichiers, puis corriger et rappeler jusqu'à ce que ça passe.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "cwd": {
                            "type": "string",
                            "description": "Dossier racine du projet à vérifier (ex: 'Pictures/montage-pc'). Optionnel : déduit des fichiers déjà écrits si absent."
                        }
                    }
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
        json!({
            "type": "function",
            "function": {
                "name": "update_tasks",
                "description": "Met à jour le plan de tâches de la demande en cours. À appeler DÈS LE DÉBUT pour lister les étapes concrètes (exploration, chaque fichier à créer/modifier, vérification), puis après CHAQUE étape pour cocher son avancement. Tu DOIS continuer à travailler tant que toutes les tâches ne sont pas 'done'. Renvoie la liste COMPLÈTE (elle remplace la précédente).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "tasks": {
                            "type": "array",
                            "description": "Liste complète et ordonnée des tâches restantes et terminées.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string", "description": "Identifiant court et unique (ex: '1', 'edit-app', 'verify')." },
                                    "title": { "type": "string", "description": "Action concrète et vérifiable (ex: 'Ajouter la route /export dans src/api.rs')." },
                                    "status": { "type": "string", "enum": ["pending", "in_progress", "done"], "description": "État de la tâche." }
                                },
                                "required": ["id", "title", "status"]
                            }
                        }
                    },
                    "required": ["tasks"]
                }
            }
        }),
    ]
}
