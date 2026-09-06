# Task 28 — Capacités Agentiques et inspection du Projet (Tool Use & RAG)

**Status:** 🔴 Not started

## Goal

Permettre à l'IA de lire l'arborescence et le contenu des fichiers du projet ouvert afin de fournir des réponses contextuelles précises sur le codebase.

## Why

Sans inspection autonome du projet, l'assistant ne peut pas comprendre la structure globale d'une application complexe ni référer des fonctions définies dans d'autres fichiers.

## Steps

- [ ] **Step 1 — Définition des outils de lecture.**
      Définir des outils (`Tools`) dans `rig-core` pour la lecture de fichier (`read_file`) et le listage de répertoire (`list_dir`).
      *Testable:* Un test unitaire simule l'appel d'outil et vérifie le retour du contenu de fichier.

- [ ] **Step 2 — Demande d'autorisation dans l'IHM.**
      Afficher une notification/demande de confirmation dans l'IHM lorsque l'IA souhaite inspecter un fichier local.
      *Testable:* Refuser l'autorisation bloque l'accès au fichier et informe l'IA.

- [ ] **Step 3 — RAG / Indexation légère du projet.**
      Parcourir le dossier racine du projet pour construire un index léger des symboles et structures de fichiers.
      *Testable:* Une recherche contextuelle injecte les fichiers pertinents dans le prompt de l'IA.

- [ ] **Step 4 — Vérification des requêtes multi-fichiers.**
      Valider le fonctionnement des requêtes contextuelles ("Explique comment la fonction X interagit avec le module Y").
      *Testable:* Un test d'intégration valide la chaîne d'appels d'outils et de réponse finale.

- [ ] **Step 5 — Vérifier que toute la suite de tests passe.**
      *Testable:* `cargo test` passe et `cargo clippy --all-targets -- -D warnings` est propre.

## Files changed

- `src/api.rs`
- `src/main.rs`
- `src/file_tree.rs`

