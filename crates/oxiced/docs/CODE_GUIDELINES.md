# Code Guidelines

- Keep widget helpers minimal and composable.
- Avoid adding runtime dependencies unless needed by multiple helpers.
- Keep public APIs typed and stable enough for downstream crates.
- Add unit tests for pure style/helper behavior when practical.
- Use `cargo fmt` and `cargo test` for verification.
- Develop shared controls in this workspace and check downstream host/plugins
  together; the root lockfile/flake defines the supported dependency graph.
- Custom wrappers need a distinct tree tag and defensive child reconciliation;
  surfaces may replace empty/stateless roots, and optional children change at runtime.
- Draw-only drag previews must not intercept events, operations or child overlays.
- Invalidate bounds caches on logical identity changes, even when widget geometry
  is unchanged. Keep label preferences caller-owned and preserve legacy helper APIs.
- Derive scheme colors into semantic roles; preserve explicit overrides and check
  automatic text against every supported interaction background. Keep contrast
  calculation pure and bounded.
- Avoid vertical `Fill` inside an unbounded scroll body. Alignment helpers that set
  `Fill` can override a previously fixed height and collapse an enclosing row.
- Compose shared focusable helpers for actions. Keep key configuration in the caller's
  navigation root and translate to canonical control keys there, including overlays.
  Use standard Iced focus/scroll operations across dylibs, not custom `Any` policy.
  Preserve unknown native focusables as editors and retain logical focus across
  leave/enter. Activate callbacks directly so nested controls keep their own targets.
