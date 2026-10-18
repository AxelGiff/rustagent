# RustAgent — Assistant de codage

> 🚧 **Statut : prototype avancé, en développement actif**

**RustAgent** est une application de bureau native qui combine un assistant de
chat IA, un éditeur de code avec coloration syntaxique, une revue de diffs et
un terminal intégré dans une seule fenêtre redimensionnable. Le projet compile
proprement, dispose d'une suite de tests unitaires (107 tests) et d'un pipeline
d'intégration continue. Il reste néanmoins un **prototype** : certaines
fonctionnalités sont partielles et le flux complet chat → éditeur → terminal
n'est pas encore validé de bout en bout de manière automatisée (voir
[`E2E_TESTING.md`](E2E_TESTING.md)).

---

## Présentation

**RustAgent** est écrit en **Rust** et repose sur le framework GUI
[**Freya**](https://github.com/marc2332/freya).

L'idée est simple : vous demandez à l'assistant (en langage naturel) d'écrire du
code. Il appelle alors des **outils natifs** (lecture/écriture de fichiers,
recherche, exécution de commandes…) ; les fichiers créés ou modifiés sont
présentés sous forme de **diffs** que vous validez avant écriture sur disque, et
les commandes shell sont soumises à **confirmation** avant exécution.

<img width="1292" height="701" alt="image" src="https://github.com/user-attachments/assets/8084ab30-cc48-4ba3-9f7d-904a774517b0" />

### Fonctionnalités

- 💬 **Panneau de chat IA** — discuter avec un LLM adossé à l'API **Albert** du
  gouvernement français (point de terminaison compatible OpenAI). Réponses en
  **streaming** (SSE), avec annulation, délais d'attente et reprises sur échec.
- 🛡️ **Mode Spec (Security by Design)** — l'agent pose d'abord quelques questions
  d'architecture/sécurité (`ask_spec_questions`), génère un `plan.md`, puis
  implémente le projet. Un **Mode Quick Edit** permet à l'inverse une exécution
  directe.
- 🧰 **Outils natifs (function calling)** — `read_file`, `write_file`,
  `write_plan`, `edit_file`, `list_directory`, `search_files`, `run_command`,
  `ask_spec_questions`, `verify_project`, `update_tasks`.
- 🧭 **Agent multi-étapes autonome** — pour une fonctionnalité, l'agent établit son
  **plan de tâches** (`update_tasks`), l'exécute étape par étape (explorer →
  modifier → vérifier) et **ne s'arrête pas** tant qu'il reste des tâches
  inachevées. S'il tente de conclure sans avoir agi, il est **relancé
  automatiquement** au lieu de vous rendre la main.
- ✏️ **Modification de code existant** — pour éditer du code, l'agent commence par
  **lire** les fichiers concernés, puis utilise `edit_file` avec un extrait exact.
  La comparaison **tolère les différences d'indentation**, et en cas d'échec le
  contenu réel du fichier est renvoyé à l'agent pour qu'il corrige et réessaie
  **dans le même tour** (fini les allers-retours). Les lectures voient aussi les
  modifications en attente du tour en cours.
- 📝 **Revue de diffs** — toute création/modification de fichier produit un diff
  en attente ; rien n'est écrit tant que vous n'avez pas accepté.
- 🔎 **Vérification automatique** — l'agent peut compiler et **tester** son propre
  travail (`cargo check`/`cargo test`, `npm run build`/`npm test`, `py_compile`,
  `go build`/`go test`…) sur une **copie de travail temporaire** du projet. Les
  commandes et leur sortie **défilent en direct** dans la conversation, façon
  terminal ; le projet réel et la revue de diffs restent intacts.
- 🔒 **Approbation des commandes (sandbox)** — les commandes en lecture seule
  sont exécutées directement ; les autres demandent une confirmation explicite,
  et certaines commandes destructrices sont bloquées. Un bouton
  « Toujours autoriser (session) » permet d'activer l'auto-approbation.
- ✍️ **Éditeur de code** — éditeur propulsé par tree-sitter (coloration
  syntaxique, thème sombre, plusieurs langages).
- 🖥️ **Terminal intégré** — terminal interactif, avec un shell et des commandes
  choisis **par plateforme**.
- 🌐 **Prise en charge multi-langages** — Python, Rust, JavaScript, TypeScript,
  HTML, CSS, C, C++, Java et Go.

---

## Structure du projet

Le code est organisé en modules thématiques ; `src/api.rs` et `src/tools.rs` ne
sont que de fins « shims » qui ré-exportent leurs sous-modules pour garder une
API stable.

```
rustagent/
├── Cargo.toml / Cargo.lock      # Manifeste + verrouillage des dépendances
├── Makefile                     # Raccourcis de développement (build, test, lint…)
├── README.md  CHANGELOG.md  RELEASING.md  E2E_TESTING.md  SPEC_SYSTEM_PLAN.md
├── .github/workflows/           # CI (ci.yml) et publication (release.yml)
├── assets/                      # Icônes et ressources d'empaquetage
└── src/
    ├── main.rs                  # Point d'entrée + composant racine (hooks Freya)
    ├── agent.rs                 # Orchestration d'un tour d'agent + construction des prompts
    ├── config.rs                # Clé API (env / fichier / panneau Paramètres) + validation
    │
    ├── api.rs                   # Shim : ré-exporte les sous-modules `api/*`
    ├── api/
    │   ├── client.rs            # Racine du client LLM
    │   ├── client/
    │   │   ├── agent_loop.rs    #   Boucle non-streaming (legacy)
    │   │   ├── policy.rs        #   Intention du tour + sélection d'outils + hygiène des messages
    │   │   ├── sse.rs           #   Accumulateur de flux SSE (texte, raisonnement, tool calls)
    │   │   └── stream_loop.rs   #   Boucle d'agent en streaming (orchestration)
    │   ├── error.rs             #   Classification des erreurs + backoff exponentiel
    │   ├── models.rs            #   Récupération de la liste des modèles
    │   ├── parser.rs            #   Historique de chat, réparation JSON tolérante
    │   ├── stream.rs            #   Primitives de streaming + drapeau d'annulation
    │   └── types.rs             #   DTO OpenAI/Albert
    │
    ├── tools.rs                 # Shim : ré-exporte les sous-modules `tools/*`
    ├── tools/
    │   ├── approval.rs          #   Approbation/sandbox des commandes (canal + UI)
    │   ├── definitions.rs       #   Schémas « function calling » des outils
    │   ├── executor.rs          #   Routeur d'exécution des outils natifs
    │   ├── extract.rs           #   Extraction tolérante de fichiers/blocs de code
    │   ├── fs.rs                #   Lecture/écriture, exécution shell, sandbox de commandes
    │   ├── plan.rs              #   Questions de spec, plans, instantané de projet
    │   ├── thinking.rs          #   Filtrage des balises `<think>…</think>`
    │   └── verify.rs            #   Compilation/tests du projet sur une copie temporaire
    │
    ├── views/                   # Composants d'interface (Freya)
    │   ├── chat.rs  command_confirm.rs  diff_viewer.rs  editor.rs
    │   ├── file_tree.rs  settings.rs  terminal.rs  toolbar.rs  workspace.rs
    │
    ├── diff.rs / myers.rs       # Calcul de diffs (algorithme de Myers via `similar`)
    ├── flow.rs                  # Règles métier : détection de langage, extraction, actions éditeur
    ├── language.rs              # Langages supportés : grammaire tree-sitter + commande d'exécution
    ├── file_tree.rs             # Modèle d'arborescence de fichiers
    ├── mcp.rs                   # Client MCP (Model Context Protocol) — expérimental
    ├── platform.rs              # Comportements spécifiques à la plateforme (shell, binaires…)
    ├── session.rs               # État de session (annulation, approbations, staging, workspaces)
    ├── theme.rs                 # Thème (« stylesheet ») chargé depuis theme.json
    └── theme.json               # Palette de couleurs
```

---

## Pour commencer

### Prérequis

- Une chaîne d'outils **Rust** récente (le manifeste cible `edition = "2024"`,
  donc Rust 1.85+).
- Les dépendances natives de **Freya** (voir la
  [documentation Freya](https://github.com/marc2332/freya) pour la configuration
  de la plateforme).
- Une **clé API Albert** (service IA du gouvernement français) — voir
  [Configuration](#configuration).
- Les environnements d'exécution des langages que vous souhaitez exécuter
  (Python, Node, GCC/G++, JDK, Go, Rust, etc.).

### Compilation et exécution

```bash
# Compiler le projet
cargo build

# Lancer l'application
cargo run
```

Un [`Makefile`](Makefile) fournit des raccourcis équivalents :

```bash
make build    # compilation (debug)
make run      # compiler puis lancer
make test     # suite de tests complète
make lint     # clippy, avertissements traités comme erreurs
make fmt      # formatage automatique
make check    # fmt-check + lint + test (identique à la CI)
make release  # binaire optimisé
```

---

## Configuration

Le point de terminaison et le modèle par défaut sont définis dans
`src/agent.rs` :

```rust
pub const ALBERT_ENDPOINT: &str = "https://albert.api.etalab.gouv.fr/v1";
pub const ALBERT_MODEL: &str = "deepseek-v4-flash";
```

Ils peuvent être surchargés via le fichier de configuration ou le panneau
**Paramètres**.

### Clé API

La clé API peut être fournie par trois canaux, par ordre de priorité :

1. **Variable d'environnement** `ALBERT_API_KEY` (priorité la plus élevée).
2. **Fichier de configuration** dans le répertoire de configuration de la
   plateforme :
   - Linux : `~/.config/rustagent/config.toml`
   - macOS : `~/Library/Application Support/rustagent/config.toml`
   - Windows : `%APPDATA%\rustagent\config.toml`
3. **Panneau Paramètres** — le bouton **Paramètres** de la barre d'outils ouvre
   un panneau où vous pouvez saisir et enregistrer la clé dans le fichier de
   configuration.

Au démarrage, l'application valide la clé et affiche un avertissement clair si
elle est manquante ou invalide. Les erreurs d'API sont classées (authentification,
limite de débit, réseau, délai d'attente, modèle) avec des messages adaptés, et
les échecs transitoires sont réessayés automatiquement avec un backoff progressif.
Chaque requête dispose d'un délai d'attente pour que l'interface ne se bloque
jamais indéfiniment.

---

## Comment cela fonctionne

1. **Chat** — saisissez un message et envoyez-le. L'agent construit un prompt
   système adapté au mode (Spec ou Quick Edit) et démarre la boucle d'agent en
   **streaming**.
2. **Appels d'outils** — le modèle répond soit en texte, soit par des **appels
   d'outils natifs**. L'agent exécute ces outils et renvoie leur résultat au
   modèle, sur plusieurs tours (avec un budget d'exploration borné).
3. **Diffs en attente** — les outils `write_file` / `edit_file` / `write_plan` ne
   touchent pas au disque directement : ils produisent un **diff** affiché dans
   l'onglet de revue, que vous acceptez ou rejetez.
4. **Approbation des commandes** — l'outil `run_command` est filtré par un
   « sandbox » :
   - les commandes manifestement destructrices sont **bloquées** ;
   - les commandes en lecture seule (inspection, versions, git en lecture) sont
     **pré-approuvées** ;
   - toutes les autres **requièrent votre confirmation** via une barre dédiée en
     bas de fenêtre (boutons *Exécuter* / *Refuser* / *Toujours autoriser*).
5. **Exécution** — le bouton **Exécuter le code** écrit le contenu de l'éditeur
   dans un fichier temporaire et le lance dans le terminal intégré à l'aide de la
   commande du langage détecté (voir ci-dessous).
6. **Effacer l'éditeur** — saisir « clear editor » (ou une formulation
   équivalente) vide l'éditeur localement sans appeler l'IA.

### Actions de la barre d'outils

| Bouton                        | Action                                             |
| ----------------------------- | -------------------------------------------------- |
| **Thème**                     | Change le thème de l'application.                  |
| **Paramètres**                | Ouvre le panneau de configuration (clé API, modèle, point de terminaison). |
| **Effacer le chat**           | Vide l'historique du chat.                         |
| **Réinitialiser le terminal** | Tue et relance le shell intégré.                   |

---

## Langages pris en charge

Chaque langage sait construire sa commande d'exécution, en utilisant les binaires
adaptés à la plateforme (`src/platform.rs`, `src/language.rs`). Les binaires
compilés sont écrits dans le répertoire temporaire de la plateforme.

| Langage    | Extension | Commande d'exécution (indicative)                         |
| ---------- | --------- | --------------------------------------------------------- |
| Python     | `.py`     | `python` (Windows) / `python3` (Unix) `<fichier>`         |
| Rust       | `.rs`     | `rustc <fichier> -o <tmp> && <tmp>`                       |
| JavaScript | `.js`     | `node <fichier>`                                          |
| TypeScript | `.ts`     | `npx ts-node <fichier>`                                   |
| HTML       | `.html`   | `start` (Windows) / `open` (macOS) / `xdg-open` (Linux)   |
| CSS        | `.css`    | *(rien à exécuter)*                                       |
| C          | `.c`      | `gcc <fichier> -o <tmp> && <tmp>`                         |
| C++        | `.cpp`    | `g++ <fichier> -o <tmp> && <tmp>`                         |
| Java       | `.java`   | `javac <fichier> && java <classe>`                        |
| Go         | `.go`     | `go run <fichier>`                                        |

---

## Tests et intégration continue

- **Tests unitaires** : `cargo test` (actuellement **131 tests** couvrant la
  détection de langage, l'extraction de code, la construction des commandes
  d'exécution, les diffs, le filtrage du raisonnement, l'approbation des
  commandes, etc.).
- **CI** : le workflow [`.github/workflows/ci.yml`](.github/workflows/ci.yml)
  exécute `cargo fmt --check`, `cargo clippy -- -D warnings` et `cargo test`.
- **Publication** : [`.github/workflows/release.yml`](.github/workflows/release.yml)
  et la configuration d'empaquetage dans `Cargo.toml` (`cargo bundle` /
  `cargo deb`) — voir [`RELEASING.md`](RELEASING.md).
- **Tests E2E manuels** : voir [`E2E_TESTING.md`](E2E_TESTING.md).

---

## Dépendances principales

- **freya** (`markdown`, `terminal`, `remote-asset`, `code-editor`) —
  framework GUI, éditeur et terminal.
- **freya-engine** — moteur de rendu sous-jacent.
- **reqwest** — client HTTP utilisé par la boucle d'agent (streaming SSE).
- **rig-core** — bibliothèque cliente LLM (utilisée pour l'historique de chat).
- **tokio** — runtime asynchrone (`rt`, `fs`, `process`, `sync`, `time`…).
- **futures-util** — utilitaires de flux.
- **ropey** — structure de données rope qui sous-tend l'éditeur.
- **tree-sitter-\*** — grammaires et requêtes de coloration pour chaque langage.
- **rio-vt** — émulation de terminal virtuel.
- **similar** — calcul de diffs (algorithme de Myers).
- **serde / serde_json / toml** — sérialisation et configuration.
- **thiserror** — types d'erreur.

---

## Feuille de route

- [x] Compilation propre dans un checkout autonome, racine d'espace de travail et
      `Cargo.lock` validés.
- [x] Configuration de la clé API (env / fichier / panneau) et classification des
      erreurs.
- [x] Prise en charge multiplateforme du terminal et des commandes d'exécution.
- [x] Outils natifs (function calling) et revue de diffs avant écriture.
- [x] Sandbox / approbation des commandes shell.
- [x] Suite de tests, CI et configuration d'empaquetage.
- [ ] Validation automatisée de bout en bout du flux chat → éditeur → terminal.
- [ ] Journalisation structurée à la place des impressions de débogage.
- [ ] Nettoyage des types/constantes morts et factorisation des boucles de reprise.

---

## Licence

Ce projet est distribué sous la licence **MIT**.

Copyright (c) 2026 **Salvatore DI DiO**

Voir le fichier [LICENSE](LICENSE) pour le texte complet de la licence.

---

## Avertissement

Il s'agit d'un projet en **développement actif**. Il est fourni à titre de
référence et d'expérimentation. Ne vous y fiez pas pour une utilisation en
production sans revue ni durcissement préalables.

---

## Remerciements

Ce code a été développé avec l'assistance de **DeepseekFlash**, un modèle fourni
via l'**API Albert** de la [DINUM](https://www.numerique.gouv.fr/numerique-etat/dinum/)
et [CsAgent](https://github.com/artydev/csagent).
