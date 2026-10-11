# Architecture

Oxiced is a Rust library for iced theme and widget helpers.

## Structure

- `src/lib.rs`: exports public modules.
- `src/theme`: legacy seed loading, computed tokens and immutable live snapshots.
- `src/widgets`: reusable iced widget wrappers and style helpers.
- `src/utils`: support helpers for color, files, layershell, focus, spacing, and macros.
- `src/tools`: internal/demo tooling.

## Widget Pattern

- Widget modules expose style functions and small builder helpers.
- Helpers use `OXITHEME` as the source of truth for colors, radius, padding, and text sizes.
- Application-specific helpers should stay generic enough to be reusable by downstream apps.
- `src/widgets/oxi_plugin.rs` contains compact helpers used by status-bar/plugin UIs: shadowless bar buttons, text role styles, and compact card containers.

## Current Downstream

- Oxibar consumes oxiced for theme colors, layer styling, buttons, sliders, picklists, text inputs, and plugin UI helpers.
- Oxiced is a local workspace member in `crates/oxiced`, sharing Oxibar's root
  lockfile, flake, Iced 0.14 and `iced_layershell` 0.17.1 dependency graph.
- Shared selectable rows and settings cards centralize theme-derived compact UI.

## Snapshot Ownership

Consumers install an explicit `Arc<ThemeSnapshot>`. No library silently polls a
mutable host global. `OXITHEME` is a synchronized compatibility handle over immutable
interned palette/font values; old field references remain valid and the 128-value
budget bounds retained storage. Revision/motion changes reuse the same intern.
The seed remains the default for legacy consumers.

Font-aware ordinary text/input/picker helpers use the snapshot family; icon glyphs
keep theirs. The host uploads fonts before installing the canonical family.
Reduced/zero motion is handled by returning final content, rather than driving an
animation with an invalid zero duration. Snapshot validation is pure and shared
with source-aware host configuration.
