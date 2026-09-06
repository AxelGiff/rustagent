# Task 25 — Rendu Markdown riche et blocs de code dans le Chat

**Status:** 🟢 Completed

## Goal

Rendre les messages de l'assistant IA avec une mise en forme Markdown dynamique (titres, listes, gras/italique, blocs de code syntaxés) au lieu de texte brut.

## Why

Le rendu Markdown riche améliore considérablement la lisibilité des réponses techniques et permet de copier facilement des extraits de code.

## Steps

- [x] **Step 1 — Intégrer le composant Markdown dans le Chat.**
      Intégrer le composant Markdown natif de Freya dans la liste des messages de chat.
      *Testable:* Les balises Markdown (ex: `**gras**`, `# Titre`) sont affichées correctement sans balises brutes visibles.

- [x] **Step 2 — Coloration syntaxique des blocs de code.**
      Activer la coloration syntaxique sur les blocs de code rendus dans le chat pour les 10 langages supportés.
      *Testable:* Un bloc de code dans une réponse affiche des couleurs de mots-clés syntaxiques.

- [x] **Step 3 — Bouton de copie du code.**
      Ajouter un bouton "Copier le code" au survol de chaque bloc de code dans le chat.
      *Testable:* Un clic sur le bouton copie le texte exact du bloc de code dans le presse-papiers.

- [x] **Step 4 — Support du streaming avec le Markdown.**
      S'assurer que le rendu Markdown et la fermeture des blocs de code s'adaptent dynamiquement pendant le flux de streaming.
      *Testable:* Le streaming n'entraîne aucun crash ni blocage d'affichage lors de la réception progressive de balises Markdown.

- [x] **Step 5 — Vérifier que toute la suite de tests passe.**
      *Testable:* `cargo test` passe et `cargo clippy --all-targets -- -D warnings` est propre.

## Files changed

- `src/main.rs`
- `src/flow.rs`

