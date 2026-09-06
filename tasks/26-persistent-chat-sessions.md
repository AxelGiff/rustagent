# Task 26 — Persistance et historique des sessions de conversation

**Status:** 🔴 Not started

## Goal

Permettre d'enregistrer, de lister, de charger et de supprimer plusieurs sessions de chat historiques sauvegardées localement sur le disque.

## Why

L'utilisateur doit pouvoir retrouver ses anciennes conversations et projets d'assistance sans tout perdre à la fermeture de l'application.

## Steps

- [ ] **Step 1 — Structure de données et stockage JSON.**
      Créer un module de persistance locale dans le dossier de configuration (`~/.config/rustagent/sessions/`).
      *Testable:* Un test unitaire sauvegarde et sérialise/désérialise une session de conversation complète.

- [ ] **Step 2 — Auto-sauvegarde des sessions.**
      Sauvegarder automatiquement la session en cours lors de chaque échange avec un titre dérivé du premier message.
      *Testable:* Un test vérifie qu'une nouvelle session génère un fichier JSON valide avec un titre approprié.

- [ ] **Step 3 — Panneau latéral d'historique des conversations.**
      Ajouter un composant d'interface latérale pour afficher et sélectionner les sessions sauvegardées.
      *Testable:* Cliquer sur une session charge correctement ses messages dans le chat.

- [ ] **Step 4 — Supression et réinitialisation de session.**
      Permettre de supprimer une session enregistrée ou d'en démarrer une nouvelle à tout moment.
      *Testable:* Supprimer une session efface le fichier JSON correspondant du disque.

- [ ] **Step 5 — Vérifier que toute la suite de tests passe.**
      *Testable:* `cargo test` passe et `cargo clippy --all-targets -- -D warnings` est propre.

## Files changed

- `src/config.rs`
- `src/main.rs`
- `src/api.rs`

