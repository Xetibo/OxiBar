# Local layer-shell backend

Imported from the published `layershellev` 0.17.1 crate, licensed under MIT.
Upstream: <https://github.com/waycrate/exwlshelleventloop>.

The local patch separates compositor keyboard focus from the most recently
created or pointer-interacted surface. Creating a non-interactive OSD must not
redirect keyboard input from an existing settings, launcher, or reply editor.

This crate remains a patched dependency rather than an application workspace
member. Cargo and Nix use the same source through the root crates.io patch.
