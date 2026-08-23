//! Colour themes as data.
//!
//! Every theme is a `const`-constructible [`Theme`] living in a static table, so adding
//! one is adding a row rather than editing five parallel lists and a `match`.

pub mod builtin;

use ratatui::style::Color;

#[derive(Debug)]
pub struct Palette {
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    pub warn: Color,
    pub ok: Color,
    pub err: Color,
    pub key: Color,
    pub brand: Color,
    pub clusters: Color,
    pub jobs: Color,
    pub pipelines: Color,
    pub warehouses: Color,
    pub catalog: Color,
    /// Header wordmark gradient endpoints.
    pub grad_from: (u8, u8, u8),
    pub grad_to: (u8, u8, u8),
    /// The terminal background this palette was designed against. Never painted —
    /// the app inherits the terminal's background — but recorded so the picker can
    /// show a swatch and so the contrast tests have something to measure against.
    pub bg: Color,
}

pub const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const fn rgb3(hex: u32) -> (u8, u8, u8) {
    ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Which terminal background a theme expects. Groups the picker; drives no logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
}

/// Where a theme came from. Groups the picker and namespaces ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    Parks,
}

#[derive(Debug)]
pub struct Theme {
    /// Stable id, the kebab-case form `--theme` accepts and `config.json` stores.
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ThemeKind,
    pub origin: Origin,
    /// Lowercase search blob the picker matches on, beyond `name` and `id`.
    pub keywords: &'static str,
    pub palette: Palette,
}

/// Every theme, in display order: built-ins first, then parks.
pub fn all() -> impl Iterator<Item = &'static Theme> + Clone {
    builtin::BUILTIN.iter()
}

pub fn count() -> usize {
    builtin::BUILTIN.len()
}

pub fn nth(i: usize) -> Option<&'static Theme> {
    all().nth(i)
}

pub fn find(id: &str) -> Option<&'static Theme> {
    all().find(|t| t.id == id)
}

/// Position of `id` in [`all`], for openers that start at the current theme.
pub fn index_of(id: &str) -> Option<usize> {
    all().position(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_is_non_empty_and_counted() {
        assert!(count() > 0);
        assert_eq!(all().count(), count());
    }

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for t in all() {
            assert!(seen.insert(t.id), "duplicate theme id: {}", t.id);
        }
    }

    #[test]
    fn find_round_trips_every_theme() {
        for t in all() {
            let found = find(t.id).expect("every id resolves");
            assert!(
                std::ptr::eq(found, t),
                "find({}) returned another row",
                t.id
            );
        }
    }

    #[test]
    fn nth_and_index_of_agree() {
        for (i, t) in all().enumerate() {
            assert_eq!(index_of(t.id), Some(i));
            assert!(std::ptr::eq(nth(i).unwrap(), t));
        }
        assert!(nth(count()).is_none());
    }

    #[test]
    fn ids_are_kebab_case() {
        for t in all() {
            assert!(
                t.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "theme id is not kebab-case: {}",
                t.id
            );
            assert!(!t.name.is_empty(), "theme {} has no name", t.id);
        }
    }

    /// The 16 ids that shipped before themes became data. They live in users'
    /// `config.json`, so none of them may be renamed — including the `gruvbox` /
    /// `gruvbox-light` inconsistency.
    #[test]
    fn legacy_builtin_ids_are_preserved() {
        for id in [
            "dark",
            "light",
            "catppuccin-mocha",
            "catppuccin-macchiato",
            "catppuccin-frappe",
            "catppuccin-latte",
            "gruvbox",
            "gruvbox-light",
            "dracula",
            "nord",
            "tokyo-night",
            "rose-pine",
            "everforest",
            "kanagawa",
            "solarized-dark",
            "one-dark",
        ] {
            assert!(find(id).is_some(), "built-in id disappeared: {id}");
        }
        assert_eq!(builtin::BUILTIN.len(), 16);
    }
}
