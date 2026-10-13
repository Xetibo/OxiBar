# Plugin SDK and lifecycle

Oxibar's plugin boundary is a **Rust dylib ABI**, not a portable C ABI. Build host
and plugins with the same pinned toolchain, Cargo lock and shared workspace
dependency graph. The `abi_version()` gate is necessary but is not a substitute
for matching Rust/iced types and features. Required ABI version remains **6**.

## Build and install

`plugins/example` is the small, separately packaged reference:

```sh
nix develop -c cargo build -p example
nix build .#oxibar-example
```

Use `crate-type = ["dylib"]`, depend on `oxibar-plugin-api`, and use its iced,
toml and model/message types. Add the package to the flake's `pluginPackage`
exports so it shares Crane artifacts with the host. Install the resulting
`libexample.so` under `$XDG_CONFIG_HOME/oxibar/plugins/`.
That user directory takes precedence over colon-separated `OXIBAR_PLUGIN_DIR`
fallback directories. The Nix host supplies its pinned Applications library as a
fallback; only explicitly enabled filenames are initialized.

`oxibar plugins` and Settings → Plugins show installed names, ABI, capabilities,
load diagnostics and enablement. Inspection reads symbols without initializing a
model or starting a subscription. Home Manager's `extraPlugins` installs disabled
catalog entries; `config.plugins` installs and enables libraries in the Nix base.
GUI changes write sparse plugin selections and take effect live. The launcher
defaults switch manages `launcher.default_plugins` for Applications/Commands/Tools; the host
loads its deduplicated union with ordinary `plugins`, without initializing every
installed library.

## Required exports

Every export uses `#[unsafe(no_mangle)] pub extern "Rust" fn ...`. Type-check
exports against the function aliases in the API, as the example does.

| Symbol | Signature / contract |
| --- | --- |
| `abi_version` | `() -> u32`, return `ABI_VERSION` |
| `name` | `() -> &'static str`, unique canonical display name |
| `model` | `(toml::Table) -> (PluginModel, Option<Task<PluginMsg>>)` |
| `update` | `(PluginModel, PluginMsg) -> Option<Task<PluginMsg>>` |
| `launch` | `(usize, PluginModel) -> Option<Task<PluginMsg>>` |
| `view` | `(PluginModel) -> Result<Vec<Element<'static, PluginMsg>>, io::Error>` |
| `errors` | `(PluginModel) -> Vec<String>`, drain transient diagnostics |
| `subscription` | `() -> *mut PluginStream`, transfer one boxed stream |

Construct models with `plugin_model` and borrow them through `with_model_read`
and `with_model_write`. Handle an unknown message or unavailable borrow without
panicking. One model and at most one active subscription belong to each loaded
provider, regardless of bar outputs or widget instances.

## Optional capabilities

Exact signatures are API aliases; missing hooks keep legacy fallbacks.

| Exports | Ownership / fallback |
| --- | --- |
| `availability`, `metadata`, `popup_metrics` | Load gate and static/dynamic popup geometry; cached native services may remain loaded while offline |
| `bar_view` | `(model, PluginViewContext, show_labels)`; output-local label preference, preferred over `view_with_context` and `view` |
| `view_with_context` | Stable bar instance/output context; falls back to `view` |
| `popup_view`, `modal_view`, `panel_view`, `toast_view` | Inner content only; host owns windows, chrome, output and routing |
| `modal_closed` | Idempotent, nonblocking cancellation exactly once per retired modal; late cleanup tasks remain valid |
| `control_center_view` | Same shared model; falls back to popup then panel |
| `desktop_view`, `desktop_interactive` | Per-instance context and automatic input policy; normal legacy widgets remain interactive, read-only bodies return false |
| `dock_entries` | Immutable `Arc<Vec<DockEntry>>` from the resident application index, with stable launch IDs |
| `compositor_changed` | One host-cached compositor snapshot, including opaque window incarnations |
| `settings_schema`, `config_changed`, `config_validate` | Typed fields, idempotent live updates and pure pre-persistence validation |
| `theme_changed` | Explicit immutable `Arc<ThemeSnapshot>`; use `export_theme_changed!()` and font-aware oxiced helpers |
| `runner_query`, `runner_entries`, `runner_cancel` | Latest-only cancellable search; stable opaque provider-local IDs |
| `runner_prefixes`, `runner_status`, `runner_favorite` | Longest-prefix routing, cached health/indexing, and one validated TextList override |
| `command`, `osd_view` | Actual completion outcome plus ordinary model message; host-owned source-scoped OSD |
| `session_menu` | Cached `Vec<SessionMenuEntry>` action/label/description/enabled data; host filters configured choices and delegates through `command` |
| `subscription_policy` | `Always` by default; `WhenVisible` pauses the owned stream when its last consumer disappears |

`BarViewFn` is additive at ABI 6. `PluginViewContext` and existing export signatures
remain unchanged. Use the supplied `show_labels` value when constructing that
view, rather than mutating a shared model: two outputs may choose different label
preferences. Keep the required `view` export for hosts without this optional hook.

## Tasks, streams and code lifetime

The host retains successfully initialized libraries until process exit. It does not hot-unload
plugins: Rust trait-object messages, task futures and cached widget callbacks can
outlive program state during executor shutdown. Live disable retains the existing
model for re-enable, closes consumers and cancels host-owned subscriptions. Pending
model updates/cleanup can finish, but disabled-provider host requests are ignored.
Re-enable reapplies current config/theme/compositor snapshots without another model.
Replacing initialized library code requires restart. Models can drop at shutdown;
their owned resources must be cleaned up. Configuration validators retain a library reference while their
background transaction may execute.

Use owned streams and worker guards. Drop must cancel waits, wake blocked owners
and join owned worker threads. The example's async-io timer is cancellable and
needs no host Tokio reactor. Native Audio/BlueZ/System/clipboard owners show
bounded queues, operations, reconnect and cancellation. Legacy resident server
workers are process-lifetime owners and are never restarted per view. Do not
spawn an unbounded worker per query, frame or reopened window.

Each dylib can have independent Tokio TLS. Do not assume a host reactor is visible
inside plugin code. Use runtime-independent async-io/zbus futures or own an
explicit backend runtime. `LatestWorker` allows one running and one latest pending
job; check its cancellation token inside bounded indexing/search loops.

Use `worker::interval` for owned periodic streams. Filesystem ownership must retain
unchanged watches and filter parent events to configured roots; the shared
`FileWatch` handles missing/recreated directories without a self-reindex feedback
loop. Keep profiling output outside watched fixtures.

## Independent application handoff

Call `launch::launch(arguments, working_directory)` from worker context; it is a
blocking facade around bounded native user-manager calls. It resolves an executable
and prefers an independent transient `app.slice` service, subscribes before
submission and waits for actual job completion. Caller-only service/activation FD
environment is not forwarded. Oxibar's packaged/development runtime graphics/font
overrides are restored to their original session values in both launch paths;
unwrapped session overrides remain intact. Transient stderr is journaled and direct
fallback inherits stderr. Never launch again after uncertain acceptance.

Definite unavailable/unsupported managers use `launch::direct`: argument vectors,
a new process group and a reaped child. That fallback does not move a process out
of an existing systemd cgroup. Apps, Commands, Tools, calendar and locker handoffs share this
implementation; only an explicit user-configured calendar command uses a shell.

## UI, commands and stable actions

Reuse oxiced theme roles and shared text/input/button/card helpers. Inner embedded
views must not create another backend owner, fixed outer surface or nested
full-height scroller. The host wraps source-surface messages so asynchronous
pairing/password tasks retain their originating output. Never infer the output
from a globally shared plugin model.

Use the SDK's string host requests, including `HostOsdRequest::into_message()` and
serialized toast requests. SDK-defined `Any` identities can differ between build
graphs; plugin-local message types are for that plugin's `update` function.
Task modals keep protocol state until completion; closing cancels only the active
operation. The host handles scoped Escape/Tab and outside-click dismissal.

Bar view callbacks enter the plugin's ordinary `update`; handle a local message
there and return a host request through `Task::done`. Task emissions and popup
messages pass through host request translation.

After confirming clipboard copy, emit
`HostPasteRequest { shortcut: "ctrl+v".into() }.into_message()` (or
`"ctrl+shift+v"`). The host dismisses interactive surfaces, waits for actual native
focus release and owns the bounded injection job. Parse the completion string with
`HostPasteRequest::parse_result` in `update`, clear busy state and retain errors.
Never send clipboard payloads in the host request, inject before copy completion,
or infer readiness from a delay. These helpers are additive at ABI 6.

Host control-center and inbox views normally share an attached bar
dropdown. Bar children report
real layout bounds; the host owns anchoring, scrolling, input and keyboard policy.
Notification `popup_view` may reuse its shared inbox while optional standalone panel
mode remains supported. Session owns a centered host overlay with large action
tiles; its provider still keeps enumeration cheap and cached. Power/logout commands
retain ordinary task confirmation and backend policy. The host captures output
context before retiring the menu and handing off to the native confirmation.

`RunnerEntry.index` identifies an immutable displayed action, not a vector slot.
Retain an identity through index reorder; allocate a new identity if its target
changes. Cancel stale queries, discard old generations, and revalidate before
activation. A removed entry must never launch its replacement. Clipboard payloads
belong only to their bounded history/results owner (an external persistent
clipboard service may retain its own private history) and must not enter
persistent usage, favorites, identity ledgers, logs or public status diagnostics.

`command` validates arguments before work and returns a `CommandOutcome` when work
finishes. Its ordinary message is applied before the IPC response. A queued request
is not completion. Use separate domain generations so one action does not discard
another domain's completion; successful action OSDs retain their source output,
while external service changes may follow current focus.

## Configuration and verification

Schemas declare full dotted paths, defaults, constraints and Live/Surface/Restart.
Never write the immutable base or serialize a full effective config as overrides.
`config_validate` is pure, thread-safe, bounded and independent of a UI model;
it is called before candidate adoption/persistence. Live hooks retain unrelated
state and restart-only values. Store usage through the private, versioned atomic
`UsageStore`, after successful initial load; do not overwrite unsupported formats.

Run pinned formatting, meaningful unit/host-flow checks and strict Clippy. Actual
dylib task/subscription checks must also work without host Tokio TLS. Use private
DBus/Wayland/hardware fixtures for integration, then the isolated smoke/release
scripts documented in [TESTING.md](TESTING.md). The reference example's shared
counter, live label, timer visibility and canceled reset are regression-tested.
The real example dylib contract runs 25 cycles, and loader tests verify catalog
inspection/availability skips do not initialize models and duplicate names load
once. [RELEASE_CHECKS.md](RELEASE_CHECKS.md) provides actual commands and the named
presentation-time, search, idle and reopen gates.
