# Task 27 — Éditeur multi-onglets et fonctionnalités d'édition avancées

**Status:** 🔴 Not started

## Goal

Faire évoluer l'éditeur de code d'un fichier unique vers une interface multi-onglets avec sauvegarde de fichiers sur disque.

## Why

Un éditeur multi-onglets permet de travailler simultanément sur plusieurs fichiers d'un même projet sans écraser le contenu précédent.

## Steps

- [ ] **Step 1 — Barre d'onglets de l'éditeur.**
      Ajouter un composant de barre d'onglets (`TabsContainer`) au-dessus du composant d'édition.
      *Testable:* Ajouter un nouvel onglet affiche son titre dans la barre et permet de basculer de l'un à l'autre.

- [ ] **Step 2 — État multi-fichiers et suivi des modifications.**
      Maintenir un état de fichiers ouverts avec statut de modification non sauvegardée (`●`).
      *Testable:* Modifier le texte d'un onglet marque le statut comme modifié.

- [ ] **Step 3 — Numérotation des lignes et marge gauche.**
      Activer et afficher les numéros de ligne sur la marge gauche de l'éditeur Freya.
      *Testable:* Le composant de numérotation s'aligne exactement avec chaque ligne du texte.

- [ ] **Step 4 — Raccourci de sauvegarde (Ctrl+S / Cmd+S).**
      Implémenter la sauvegarde sur disque du fichier de l'onglet actif.
      *Testable:* Déclencher la sauvegarde écrit le contenu exact sur le système de fichiers.

- [ ] **Step 5 — Vérifier que toute la suite de tests passe.**
      *Testable:* `cargo test` passe et `cargo clippy --all-targets -- -D warnings` est propre.

## Files changed

- `src/main.rs`
- `src/flow.rs`

