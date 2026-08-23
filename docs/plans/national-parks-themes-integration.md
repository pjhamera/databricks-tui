# Integrate the national-parks-themes palettes into databricks-tui

## Handoff

No code has been written yet — this plan is the whole deliverable so far. It gets committed
to `databricks-tui` as `docs/plans/national-parks-themes-integration.md` on branch
**`feature/national-park-themes-integration`** (chosen by the user, replacing this session's
default `claude/national-parks-themes-integration-s0cjxo`), so the work can continue from a
local checkout.

To pick it up on the laptop:

```
git fetch origin feature/national-park-themes-integration
git checkout feature/national-park-themes-integration
git clone https://github.com/pjhamera/national-parks-themes ../national-parks-themes
```

The parks repo is a *build-time* input only — the generator reads it from
`../national-parks-themes` by default. Nothing from it is vendored except the generated
`src/theme/parks.rs`.

## Context

`databricks-tui` ships 16 colour themes today. `pjhamera/national-parks-themes` is a
Neovim colourscheme collection covering all 63 US National Parks, authored by the same
person, with a dark palette per park and a mathematically derived light variant — 126
schemes in total.

**Short answer: yes, this makes sense — the two colour models line up almost exactly.**
Every park file declares precisely ten anchors (`bg fg accent red orange yellow green
cyan blue purple`), and `databricks-tui`'s `Palette` needs thirteen colours plus two
gradient endpoints that are all drawn from that same set. The mapping is mechanical and
lossless:

| `Palette` field | parks source |
|---|---|
| `text` | `fg` (contrast-floored to 7.0 against `bg`) |
| `dim` | `fg_muted` = `mix(fg, bg, 0.32)`, floored to 4.5 |
| `border` | `mix(fg, bg, 0.62)` |
| `err` / `ok` / `warn` / `key` | `red` / `green` / `yellow` / `cyan` |
| `brand` | `accent` (the park's signature colour) |
| `clusters` / `jobs` / `pipelines` / `warehouses` / `catalog` | `cyan` / `purple` / `green` / `blue` / `orange` |
| `grad_from` / `grad_to` | `accent` / `orange` |

That the parks set carries a real `orange` and a distinct `accent` is what makes this
work — the pre-generated terminal-emulator exports in that repo *cannot* be used as the
source, because ANSI has no slot for either.

What does **not** survive the jump is the current theme plumbing. Sixteen themes are held
in five hand-maintained parallel lists and a 280-line `match`; at 142 themes that becomes
~2,400 lines of `match` arms, a clap `ValueEnum` with 142 variants, and a `t` key that
blindly cycles through 142 palettes. So the work is: **restructure the theme system to be
data-driven, generate the parks palettes, and add a searchable picker.**

### Decisions taken
- Ship **all 126** parks themes (63 parks × dark + light).
- `t` opens a **searchable picker overlay**; blind cycling is retired.
- Colours land via a **committed generator script + committed generated Rust data** — no
  new Rust dependencies, no colour math in the binary.

### One thing to know about light themes
`src/ui.rs` calls `.bg(` exactly twice — the app never paints a background, it inherits
the terminal's. The existing `light`, `catppuccin-latte` and `gruvbox-light` themes are
therefore already "use this when your terminal is light", and the 63 parks light variants
inherit that contract. This is not a blocker, but it must be documented rather than left
for users to discover. It also gives the integration a nice story: the parks repo ships
matching emulator schemes for ghostty/kitty/alacritty/wezterm/iTerm2/Windows Terminal, so
`parks-yosemite-light` in the terminal + `--theme parks-yosemite-light` in the TUI is one
coherent look.

---

## Work

### 1. Move `Palette` into a new `src/theme/` module and make it const-constructible

`Palette` (`src/ui.rs:16-33`) and the `rgb` / `rgb3` helpers (`src/ui.rs:35-41`) move to
`src/theme/mod.rs` and become `pub`. `Palette` gains one field:

```rust
/// The terminal background this palette was designed against. Never painted —
/// the app inherits the terminal's background — but recorded so the picker can
/// show a swatch and so the contrast tests have something to measure against.
pub bg: Color,
```

`Dark` records `bg: Color::Reset`. `Color::Rgb` and the unit variants are usable in
`const` initialisers, so no other change is needed to hold palettes in a `const` table.

New type replacing the theme enum:

```rust
pub struct Theme {
    pub id: &'static str,        // "parks-yosemite"
    pub name: &'static str,      // "Yosemite"
    pub kind: ThemeKind,         // Dark | Light — groups the picker, drives no logic
    pub origin: Origin,          // Builtin | Parks
    pub keywords: &'static str,  // lowercase search blob: "yosemite california granite falls"
    pub palette: Palette,
}
```

### 2. Reduce `ThemeMode` to a handle, keeping the name to limit churn

`ThemeMode` (`src/app.rs:8-99`) loses its 16 variants, `ALL`, and the `name()` / `id()`
match arms, becoming a `Copy` newtype over a static reference:

```rust
#[derive(Debug, Clone, Copy)]
pub struct ThemeMode(&'static Theme);
```

- `PartialEq` compared by `id` (hand-written, not derived).
- `name()`, `id()` forward to the inner `Theme`; `palette()` returns `&'static Palette`.
- `from_id(&str)` scans `theme::all()`.
- `default()` → the `dark` built-in.
- `toggled()` is **retired** — nothing cycles blindly any more.

Keeping the type name means `App.theme: ThemeMode` (`src/app.rs:579`),
`App::new(refresh_secs, theme)` (`src/app.rs:713-716`) and `persist_theme()`
(`src/app.rs:967-970`, still `self.theme.id()`) need no signature changes.

In `src/ui.rs`, `fn palette(mode)` (`43-320`) is deleted and `draw()` (`343`) becomes
`let p = app.theme.palette();`. Every `&Palette` threaded through the draw functions
already takes a reference, so `&'static Palette` drops straight in — the ~40 draw-function
signatures are untouched. `accent()` (`src/ui.rs:322-332`) is untouched.

### 3. Convert the 16 built-ins to data — `src/theme/builtin.rs`

A mechanical transcription of the existing `match` arms into
`pub static BUILTIN: &[Theme] = &[...]`, preserving every current id **including the
`gruvbox` / `gruvbox-light` inconsistency** — that id is in users' `config.json`, so it
stays as-is.

`src/theme/mod.rs` exposes the two tables as one sequence:

```rust
pub fn all() -> impl Iterator<Item = &'static Theme> + Clone {
    builtin::BUILTIN.iter().chain(parks::PARKS.iter())
}
pub fn count() -> usize;
pub fn nth(i: usize) -> Option<&'static Theme>;
pub fn find(id: &str) -> Option<&'static Theme>;
```

### 4. Generate `src/theme/parks.rs` — `scripts/gen_parks_themes.py`

Python 3 stdlib only (3.11 confirmed available). Reads a local checkout of the parks repo
(`--parks-repo <path>`, default `../national-parks-themes`) and writes `src/theme/parks.rs`.

Steps:
1. Parse `lua/parks/palette/registry.lua` for the 63 `{ slug, name, dir, states }` entries.
2. Parse each `lua/parks/parks/<dir>/<slug>.lua` for the ten `dark` anchors plus `name`,
   `states`, `blurb` (regex is sufficient — the files are flat `key = "#rrggbb"` tables and
   all 63 were verified to share exactly this shape, with **zero** `light = {}` overrides).
3. Reimplement only the oklch subset needed, ported from
   `lua/parks/palette/oklch.lua`: hex↔sRGB, sRGB↔oklab, `to_lch` / `to_hex` **with the
   gamut-clipping search**, `mix`, `luminance`, `contrast` (WCAG 2.1), `ensure_contrast`
   (steps lightness by ±0.02 up to 50 times toward the contrasting direction, returns the
   best found — it is best-effort, *not* a guarantee).
   **`mix` interpolates in oklab, not oklch** — getting this wrong silently shifts every
   derived tone.
4. Reproduce `derive_light` from `lua/parks/palette/build.lua` exactly: `bg` → `l=0.955`,
   `c=min(c*2.2, 0.020)`; `fg` → `l=0.30`, `c=min(c, 0.055)`; every other anchor →
   `l = 0.52 - (l - mean_l) * 0.85`, `c = c * 1.12`, where `mean_l` is the mean oklch
   lightness of all non-`bg`/`fg` anchors.
5. Then, per variant, in this order: `text = ensure_contrast(fg, bg, 7.0)`;
   `dim = ensure_contrast(mix(fg, bg, 0.32), bg, 4.5)`; each accent =
   `ensure_contrast(anchor, bg, 4.5)`; `border = mix(fg, bg, 0.62)`.
6. Emit `pub static PARKS: &[Theme] = &[...]`, 126 entries, dark then light per park,
   registry order. Header carries `// @generated by scripts/gen_parks_themes.py — do not edit`,
   the command to regenerate, and the parks-repo commit SHA as
   `pub const PARKS_SOURCE_REV: &str = "…";` so drift is detectable.

Generator assertions (fail the run, don't warn):
- exactly 63 parks in the registry, each with a palette file and all ten anchors;
- 126 emitted rows, ids unique and matching `^parks-[a-z0-9-]+(-light)?$`;
- every emitted `text`/`bg` pair ≥ 4.5 and every `dim`/`bg` pair ≥ 3.0 (a floor sanity
  gate, deliberately below the 7.0 target that `ensure_contrast` only best-efforts);
- print a per-theme achieved-contrast report so the numbers are reviewable in the PR.

**The oklch port does not have to be trusted — it can be verified exactly.** The parks repo
commits fully-resolved exports for all 126 schemes, light ones included, and upstream's
`build_ansi` puts the contrast-floored anchors straight into the bright ANSI slots. So the
generator gets a `--verify` mode (run as part of every regeneration, failing on any
mismatch) that diffs each generated theme against `terminal/ghostty/parks-<slug>[-light]`:

| generated field | ghostty key |
|---|---|
| `bg` / `text` / `dim` | `background` / `foreground` / `palette 7` |
| `border` | `split-divider-color` |
| `err` `ok` `warn` `key` | `palette 9` / `10` / `11` / `14` |
| `jobs` / `warehouses` | `palette 13` / `12` |

Confirmed by hand on both Yosemite variants: e.g. dark `red` anchor `#d56956` is exactly
ghostty `palette 9`, and light `foreground` `#2b2e33` / `dim` `#62666e` / divider `#9ba1ab`
are all values only `derive_light` + `ensure_contrast` can produce. Eleven of the thirteen
`Palette` fields are pinned this way in **both** variants, which makes the light derivation
— the part with no other safety net — machine-checkable.

The two exceptions: `brand` (`accent`) and `catalog` (`orange`) have no ANSI slot, so they
come from the Lua anchors directly. `accent` is loosely cross-checkable against
`cursor-color` (which is `accent_bright` = `l + 0.09*dir`, `c * 1.06`); light `orange` has
no upstream cross-check at all. Both ride the same `derive_light` code path as the eleven
verified fields, so agreement there is strong evidence for these two.

Ids and names: `parks-yosemite` / "Yosemite" and `parks-yosemite-light` / "Yosemite Light".
No "Parks " display prefix — the picker groups by `Origin` instead, and the `parks-` id
prefix already namespaces them against future built-ins.

### 5. Replace the clap `ThemeArg` enum

`ThemeArg` and `From<ThemeArg> for ThemeMode` (`src/main.rs:38-79`) are deleted.
`--theme` becomes `Option<String>`, resolved via `ThemeMode::from_id` in the existing
precedence chain at `src/main.rs:104-113` (`--theme` → `config.theme` → `dark`). An
unknown value exits with a clear error naming `--list-themes` rather than dumping 142 ids
into `--help`; the flag's help text says `theme id (see --list-themes)`.

Add `--list-themes`: prints `id` + name grouped by origin and kind, then exits. This also
gives the README something to point at instead of an enumeration.

While here: an unknown id in `config.json` currently falls back to `dark` **silently**
(`src/config.rs:37-42` swallows everything). Surface it as a startup flash message.

### 6. Searchable theme picker, modelled on `Jump`

The Ctrl+P `Jump` overlay is the template and should be followed closely rather than
reinvented — state struct at `src/app.rs:696`, methods `open_jump`/`jump_matches`/
`jump_push`/`jump_pop`/`jump_next`/`jump_prev`/`jump_go` at `src/app.rs:1089-1170`,
renderer `draw_jump` at `src/ui.rs:881-947`.

New in `src/app.rs`:

```rust
pub struct ThemePicker {
    pub query: String,
    pub index: usize,
    /// Restored on Esc, so browsing with live preview is non-destructive.
    previous: ThemeMode,
}
pub theme_picker: Option<ThemePicker>,   // alongside `jump`
```

with `open_theme_picker` (opens at the current theme's index), `theme_matches()`
(lowercase substring over `name`, `id` and `keywords` — so "california" finds Yosemite and
"granite" finds it too), `theme_push`/`theme_pop`, `theme_next`/`theme_prev` (each applies
the highlighted theme immediately — live preview, no persist), `theme_confirm` (persists
via the existing `persist_theme()`), `theme_cancel` (restores `previous`, no persist).

`draw_theme_picker` in `src/ui.rs` copies `draw_jump`'s shape: centred `Clear`ed popup,
`❯ query▏` prompt line, filtered `List` with `ListState` + `REVERSED` highlight,
`∅ nothing matches` empty state. Two borrowings from `draw_picker` (`src/ui.rs:1331-1375`):
the `●` current-theme marker, and per-row swatch spans built from the row's own palette
(`brand`/`ok`/`warn`/`err`) so the list previews colours without applying them.

Key handling: `t` calls `open_theme_picker()` instead of `toggled()` at
`src/main.rs:920-926`; a new `else if app.theme_picker.is_some()` branch in the event
match handles chars/Backspace/↑↓/Enter/Esc, following the `app.picker` branch at
`src/main.rs:389-410` and the `app.wh_picker` branch at `518-538`.

Hints to update: footer `src/ui.rs:3455-3456` and help overlay `src/ui.rs:769`
("cycle color theme" → "search color themes").

### 7. Tests

There are none for themes today. Repo style is inline `#[cfg(test)] mod tests` at the
bottom of each source file (`src/ui.rs:3946+`, `src/app.rs:4341+`).

In `src/theme/mod.rs`:
- `all()` is non-empty and every `id` is unique;
- `find(t.id()) == Some(t)` round-trips for every theme;
- `PARKS.len() == 126`, and every parks id matches the expected pattern;
- every parks theme's `text` clears 4.5:1 and `dim` clears 3.0:1 against its recorded
  `bg` — this is the regression guard on the generator, and the reason `Palette` gained
  `bg`. Needs a small WCAG contrast helper in the test module.

In `src/app.rs`: `theme_matches()` returns everything on an empty query, filters on name,
id and keyword ("california" → Yosemite), and `theme_next`/`theme_prev` clamp at the ends
rather than wrapping past them; `theme_cancel` restores the pre-open theme.

### 8. Docs

- `README.md:133-138` — replace "Sixteen color themes" + the full enumeration. Keep the 16
  built-ins listed, then one line: all 63 US National Parks in dark and light, `t` to
  search, `--list-themes` to print them all. Link the parks repo.
- `README.md:192-196` (`--theme` block) and `README.md:240` (the `t` key row).
- `docs/index.html:236` — **already stale** ("eight color themes"); also `257-258`.
- `docs/troubleshooting.md:61` — note that an unknown `theme` id now flashes a warning.
- A short note (README, near the theme section) that light themes assume a light terminal
  background, pointing at the parks repo's emulator exports for the matching pair.
- `CHANGELOG.md` — new entry (the 8→16 entry at `59-62` is the precedent).
- `demo/demo.tape:99` — **already stale** ("Eight built-in themes"); `100-105` presses `t`
  three times, which now opens the picker and must be re-recorded as a picker interaction.

### 9. Sequencing

Each step compiles and is independently reviewable:

1. **Restructure only** — `src/theme/` module, `Palette` moved and given `bg`, `ThemeMode`
   as a handle, 16 built-ins as data, `ThemeArg` replaced by a validated string,
   `--list-themes`, round-trip tests. Behaviour identical; `t` still cycles the 16.
2. **Generator + generated data** — `scripts/gen_parks_themes.py`, `src/theme/parks.rs`,
   contrast tests. 142 themes reachable via `--theme`; `t` cycling now visibly inadequate,
   which is what step 3 fixes.
3. **Picker** — overlay, key handling, `toggled()` removed, footer/help text.
4. **Docs + changelog + demo tape.**

Steps 1 and 2 are shippable on their own if the picker slips.

## Verification

- `cargo build` and `cargo test` at each step; `cargo clippy -- -D warnings` if the repo
  gates on it.
- Re-run `scripts/gen_parks_themes.py` and confirm `git diff --exit-code src/theme/parks.rs`
  is clean — proves the generator is deterministic and the committed file is current.
- `scripts/gen_parks_themes.py --verify` must pass with zero mismatches across all 126
  schemes (the table in step 4). This is the real correctness gate on the oklch port: it
  compares against files upstream generated with its own Lua, so a drifted `mix`, a missing
  gamut clip, or a wrong `derive_light` constant shows up as an exact-value diff rather than
  as a colour nobody notices.
- `cargo run -- --list-themes | wc -l` → 142 themes.
- `cargo run -- --theme parks-yosemite`, then `t`: type "yos", check live preview as ↑/↓
  move, Enter persists (confirm `~/.config/databricks-tui/config.json` holds
  `"theme": "parks-yosemite"`), reopen and Esc restores the prior theme.
- `cargo run -- --theme parks-nope` → clear error pointing at `--list-themes`.
- Eyeball a few light variants in a light terminal (nobody has looked at the derived output
  yet — this is the least-proven part of the change).
