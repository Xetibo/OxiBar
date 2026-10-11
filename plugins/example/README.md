# SDK example

Build with the workspace's pinned toolchain and `oxibar-plugin-api`:

```sh
nix develop -c cargo build -p example
nix build .#oxibar-example
```

Install `libexample.so` into `$XDG_CONFIG_HOME/oxibar/plugins/`. Enable it through
Settings → Plugins and restart, or list it in the immutable base configuration.
It is packaged separately and is not enabled by default.

The example reuses one counter across its bar, popup, control center and any
number of desktop instances. Its owned timer pauses while no view is visible.
Live label settings preserve the counter; the modal's close hook cancels only
the unsubmitted reset. `oxibar action Example increment` reports completion
after the ordinary message updates the shared model. `reset` opens confirmation.
`popup` opens its existing popup and `osd` requests a source-scoped read-only counter
OSD. `example.available = false` demonstrates a restart-only availability skip;
catalog inspection does not initialize a model.

Use [the SDK guide](../../docs/PLUGIN_SDK.md) for required signatures, optional
capabilities, task/stream ownership, theme snapshots and stable runner actions.
