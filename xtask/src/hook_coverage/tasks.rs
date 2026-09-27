//! A surface's `reached_by` must be a recipe the justfile has.
//!
//! Its own module for `max-lines`' reason: the parent sits at the 1000-line cap, which cannot
//! exempt anything under `xtask/`.

use super::SURFACES;

/// The other direction of `unknown_hook_ids`: `surface_gaps` asks only whether the `reached_by`
/// task RAN, so a typo or a renamed recipe there left the surface uncovered with no complaint.
pub(super) fn unknown_reached_by_tasks(root: &std::path::Path) -> Vec<String> {
    let Some(recipes) = crate::tasks::recipe_names(root) else {
        return vec![String::from(
            "the justfile could not be read, so no surface's `reached_by` task could be checked",
        )];
    };
    SURFACES
        .iter()
        .filter(|surface| !recipes.contains(surface.reached_by))
        .map(|surface| {
            format!(
                "the `{}` surface names `reached_by = {}`, which is not a recipe in the justfile",
                surface.label, surface.reached_by
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_reached_by_naming_a_recipe_the_justfile_does_not_have_is_flagged() {
        let root = std::env::temp_dir().join(format!("sutura-hook-coverage-tasks-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(&root).expect("fixture root");
        std::fs::write(root.join("justfile"), "lint:\n    echo hi\n").expect("justfile");
        let problems = super::unknown_reached_by_tasks(&root);
        drop(std::fs::remove_dir_all(&root));
        assert!(
            problems.iter().any(|p| p.contains("`reached_by = chart`")),
            "a surface naming a recipe the justfile does not have must be flagged: {problems:?}"
        );
        assert!(
            !problems.iter().any(|p| p.contains("`reached_by = lint`")),
            "a real recipe must not be flagged: {problems:?}"
        );
    }
}
