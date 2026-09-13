use crate::diff::{computed_diff, DiffLine};

/// Print Myers algorithm diff between `old_text` and `new_text` in the console.
///
/// # Paramètres
/// - `target_name` : le nom de la cible (fichier, variable, etc.) affiché dans l'en-tête
/// - `old_text`    : le texte d'origine (avant modification)
/// - `new_text`    : le nouveau texte (après modification)
///
/// # Comportement
/// Cette fonction calcule et affiche dans la console un diff basé sur l'algorithme de Myers,
/// en distinguant les lignes inchangées, ajoutées et supprimées, puis affiche un résumé chiffré.
pub fn log_myers_diff(target_name: &str, old_text: &str, new_text: &str) {
    // --- En-tête du rapport ---------------------------------------------------
    println!("\n========================================");
    println!("[MYERS DIFF] Modifications on `{}`:", target_name);
    println!("----------------------------------------");

    // Calcule la liste des lignes différenciées via l'algorithme de Myers
    let diffs = computed_diff(old_text, new_text);

    // Compteurs pour le résumé statistique du diff
    let mut added_count = 0;      // nombre de lignes ajoutées
    let mut deleted_count = 0;    // nombre de lignes supprimées
    let mut unchanged_count = 0;  // nombre de lignes inchangées

    // Parcourt chaque ligne du résultat du diff pour l'afficher avec son préfixe
    for line in &diffs {
        match line {
            // Ligne présente dans les deux textes (inchangée) -> préfixe espace
            DiffLine::Unchanged(content) => {
                unchanged_count += 1;
                println!("  {}", content);
            }
            // Ligne présente uniquement dans le nouveau texte -> préfixe "+"
            DiffLine::Added(content) => {
                added_count += 1;
                println!("+ {}", content);
            }
            // Ligne présente uniquement dans l'ancien texte -> préfixe "-"
            DiffLine::Deleted(content) => {
                deleted_count += 1;
                println!("- {}", content);
            }
        }
    }

    // --- Pied de page : résumé chiffré des modifications ---------------------
    println!("----------------------------------------");
    println!(
        "Summary: +{} line(s), -{} line(s), {} unchanged line(s)",
        added_count, deleted_count, unchanged_count
    );
    println!("========================================\n");
}