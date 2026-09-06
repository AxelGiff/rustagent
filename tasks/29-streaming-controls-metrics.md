# Task 29 — Contrôle du Streaming & Indicateurs de Performance

**Status:** 🔴 Not started

## Goal

Donner un contrôle total à l'utilisateur pendant la génération de réponse de l'IA (bouton Stop) et afficher des indicateurs de vitesse et de statut en temps réel.

## Why

Les utilisateurs ont besoin d'interrompre une réponse inutile ou trop longue et d'avoir un retour visuel clair pendant les temps d'attente du modèle.

## Steps

- [ ] **Step 1 — Bouton "Arrêter la génération" (Stop Stream).**
      Ajouter un bouton "Stop" dans l'interface de chat actif uniquement lorsqu'une réponse est en cours de streaming.
      *Testable:* Cliquer sur "Stop" déclenche immédiatement l'annulation du flux et conserve le message partiel.

- [ ] **Step 2 — Indicateur d'état avant streaming (Thinking/Spinner).**
      Afficher une animation d'attente visuelle claire dès l'envoi de la requête jusqu'à la réception du premier token.
      *Testable:* L'animation disparaît dès que le premier token arrive.

- [ ] **Step 3 — Calcul et affichage du débit de génération.**
      Mesurer les tokens/seconde pendant le flux et afficher cette métrique sous le message de l'assistant (ex: `42.5 tokens/s`).
      *Testable:* Le calcul reflète précisément la vitesse de réception des tokens.

- [ ] **Step 4 — Gestion propre de l'interruption.**
      Formatage clair du message arrêté à la demande de l'utilisateur avec la mention `[Génération interrompue]`.
      *Testable:* Le chat reste stable et prêt pour une nouvelle question après interruption.

- [ ] **Step 5 — Vérifier que toute la suite de tests passe.**
      *Testable:* `cargo test` passe et `cargo clippy --all-targets -- -D warnings` est propre.

## Files changed

- `src/api.rs`
- `src/main.rs`

