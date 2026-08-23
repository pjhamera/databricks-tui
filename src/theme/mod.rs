//! Colour themes as data.
//!
//! Every theme is a `const`-constructible [`Theme`] living in a static table, so adding
//! one is adding a row rather than editing five parallel lists and a `match`.

pub mod builtin;
pub mod parks;

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
    builtin::BUILTIN.iter().chain(parks::PARKS.iter())
}

pub fn count() -> usize {
    builtin::BUILTIN.len() + parks::PARKS.len()
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

    /// sRGB channel to linear light (WCAG 2.1).
    fn to_linear(c: f64) -> f64 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: Color) -> Option<f64> {
        let Color::Rgb(r, g, b) = c else {
            // The `dark` built-in inherits the terminal's colours, so there is
            // nothing to measure.
            return None;
        };
        let f = |v: u8| to_linear(f64::from(v) / 255.0);
        Some(0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b))
    }

    fn contrast(a: Color, b: Color) -> Option<f64> {
        let (la, lb) = (luminance(a)?, luminance(b)?);
        let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
        Some((hi + 0.05) / (lo + 0.05))
    }

    #[test]
    fn parks_ships_all_63_in_both_variants() {
        assert_eq!(parks::PARKS.len(), 126);
        let light = parks::PARKS
            .iter()
            .filter(|t| t.kind == ThemeKind::Light)
            .count();
        assert_eq!(light, 63, "every park needs a light variant");
        assert!(parks::PARKS.iter().all(|t| t.origin == Origin::Parks));
    }

    #[test]
    fn parks_ids_are_namespaced_and_paired() {
        for t in parks::PARKS {
            assert!(
                t.id.starts_with("parks-"),
                "unnamespaced parks id: {}",
                t.id
            );
            let is_light = t.id.ends_with("-light");
            assert_eq!(
                is_light,
                t.kind == ThemeKind::Light,
                "{} id and kind disagree",
                t.id
            );
        }
    }

    /// The regression guard on scripts/gen_parks_themes.py: a drifted colour
    /// transform shows up here as an unreadable palette. The floors are
    /// deliberately below the 7.0/4.5 the generator targets, because
    /// `ensure_contrast` is best-effort rather than a guarantee.
    #[test]
    fn every_parks_theme_stays_readable_on_its_own_background() {
        for t in parks::PARKS {
            let p = &t.palette;
            let text = contrast(p.text, p.bg).expect("parks palettes are all true colour");
            assert!(
                text >= 4.5,
                "{}: text contrast {:.2} is below 4.5",
                t.id,
                text
            );
            let dim = contrast(p.dim, p.bg).expect("parks palettes are all true colour");
            assert!(dim >= 3.0, "{}: dim contrast {:.2} is below 3.0", t.id, dim);
        }
    }

    #[test]
    fn parks_source_rev_is_recorded() {
        assert_eq!(
            parks::PARKS_SOURCE_REV.len(),
            40,
            "PARKS_SOURCE_REV should be a full git sha"
        );
    }
}
