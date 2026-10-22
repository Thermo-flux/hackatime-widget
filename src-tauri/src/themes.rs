//! The theme list the tray menu offers. The themes themselves live in the UI (`ui/js/themes/data.js`);
//! the test below keeps this list from drifting from it.

/// `(id, label)` in picker order.
pub const THEMES: [(&str, &str); 13] = [
    ("classic", "Classic"),
    ("avocado", "Avocado"),
    ("tomato", "Tomato"),
    ("cat", "Cat"),
    ("dog", "Dog"),
    ("neon", "Neon"),
    ("frog", "Frog"),
    ("ghost", "Ghost"),
    ("mushroom", "Shroom"),
    ("boba", "Boba"),
    ("phosphor", "Terminal"),
    ("penguin", "Penguin"),
    ("jelly", "Jelly"),
];

pub fn is_theme(id: &str) -> bool {
    THEMES.iter().any(|(t, _)| *t == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: &str = include_str!("../../ui/js/themes/data.js");

    #[test]
    fn the_tray_lists_exactly_the_themes_the_ui_has() {
        let ids: Vec<&str> = DATA
            .lines()
            .filter_map(|l| l.strip_prefix("    id: \"")?.split('"').next())
            .collect();
        let ours: Vec<&str> = THEMES.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, ours);
    }

    #[test]
    fn the_labels_match_the_pickers_short_names() {
        let block = DATA.split("const LABELS = {").nth(1).unwrap().split("};").next().unwrap();
        for (id, label) in THEMES {
            assert!(block.contains(&format!("{id}: \"{label}\"")), "{id} should be labelled {label}");
        }
    }
}
