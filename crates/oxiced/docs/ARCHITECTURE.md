# Architecture

Oxiced is a Rust library for iced theme and widget helpers.

## Structure

- `src/lib.rs`: exports public modules.
- `src/theme`: theme loading and computed theme colors.
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
