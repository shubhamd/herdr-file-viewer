# Summoning the viewer

How the viewer gets opened: the open actions, the idempotent launcher, split vs. tab, the
`file://` link handler, and the `--remote` caveat. For a quick "install then bind a key," see the [Quick start](../README.md#quick-start);
once it's open, see the [usage guide](usage.md) and [keys reference](keys.md).

The viewer opens **only** in response to an explicit action. There are no event hooks and no
automatic invocation. The manifest declares a `[[panes]]` entry (the split-pane viewer) and an
`[[actions]]` whose command opens it:

```toml
[[panes]]
id = "file-viewer"
placement = "split"
command = ["./target/release/herdr-file-viewer"]

[[actions]]
id = "open-file-viewer"
title = "Open file viewer"
command = ["bash", "scripts/open-file-viewer.sh"]   # opens the pane via the herdr CLI
```

Summon it by invoking the action:

```bash
herdr plugin action invoke open-file-viewer --plugin herdr-file-viewer
```

It opens the viewer in a **split** pane beside your current work. The launcher
(`scripts/open-file-viewer.sh`, used by both the action and any keybinding) is **idempotent**,
scoped to the current tab, so invoking it repeatedly is *launch-or-focus-or-toggle*:

- no viewer pane open in this tab → open a split (focused)
- a viewer pane open but not focused → focus it
- the viewer pane already focused → close it (herdr has no hide-without-close; reopening just
  re-walks the tree)

**One-press access: bind a key.** herdr's `config.toml` binds keys to commands; point a
`plugin_action` binding at the installed plugin's qualified action id. herdr invokes the action
directly, so no detached shell or hard-coded path is involved:

```toml
[[keys.command]]
key = "prefix+f"   # any herdr key syntax, e.g. ctrl+b then f
type = "plugin_action"
command = "herdr-file-viewer.open-file-viewer"
description = "open file viewer in split"
```

Reload with `herdr server reload-config`. Pressing the key then opens / focuses / hides the
viewer via the same idempotent launcher.

## Split beside or below

By default the split opens to the **right** of the pane you pressed the key in. Set
[`open_direction`](configuration.md) to put it **below** instead, so your terminal keeps the top
half and the viewer takes the bottom:

```toml
# <plugin config dir>/config.toml   (`herdr plugin config-dir herdr-file-viewer`)
open_direction = "down"
```

No reload is needed — the launcher reads it on each summon, so the next `prefix+f` opens
underneath. It applies to the split action only: a tab has no direction, so
`open-file-viewer-tab` ignores it.

## Open in a tab instead of a split

A second action, `open-file-viewer-tab`, opens the viewer in its **own tab**
(`scripts/open-file-viewer-tab.sh`, `--placement tab`). Its launcher is idempotent *across the tabs
of the current workspace*, *open-or-switch-or-toggle*:

- no viewer in this workspace → open it in a new tab (focused)
- a viewer in another tab of this workspace → **switch to that tab** (never a duplicate)
- a viewer in the current tab, not focused → focus it in place
- the viewer already focused → close it (herdr auto-closes the emptied tab)

The idempotency is scoped to the **current workspace**: a viewer already open in a *different*
workspace is left where it is, and a fresh one opens here. The action reaches this workspace's
viewer, it never pulls you across workspaces.

Bind it to its own key, e.g. `prefix+shift+f` alongside `prefix+f` for the split:

```toml
[[keys.command]]
key = "prefix+shift+f"
type = "plugin_action"
command = "herdr-file-viewer.open-file-viewer-tab"
description = "open file viewer in tab"
```

## Open a file link

The manifest also declares a herdr **link handler**: a **Ctrl+click** on a `file://` link in any
pane runs the plugin's `open-file-link` action instead of herdr's default URL opener, and the viewer
opens on that file (at its line for `file:///…/app.rs:42`, `…#L42`, or `…#L42-L58`):

```toml
[[actions]]
id = "open-file-link"
title = "Open file link in file viewer"
command = ["bash", "scripts/open-file-link.sh"]

[[link_handlers]]
id = "file-link"
pattern = "^file://"        # a Rust regex herdr matches against the clicked URL
action = "open-file-link"   # must name an action this plugin declares
```

The launcher (`scripts/open-file-link.sh`) asks the viewer binary to convert the clicked URL into an
open target (`herdr-file-viewer --link-target`, which reads herdr's `HERDR_PLUGIN_CLICKED_URL`:
percent-decoding, a local-host check, and the `#L42` / `:42` anchor conventions live in that one
tested place), then opens a **fresh split** beside the clicked pane with
`--env HERDR_FILE_VIEWER_OPEN=<target>`, honoring [`open_direction`](configuration.md). It is
launch-only, like every open target: an already-open Files pane is left alone (it may hold your
annotations), and each click opens its own pane. A link that is not a local file (a remote
`file://host/…`, a bad encoding) opens nothing. The viewer roots at the clicked pane's repository,
so a path outside it is refused with a soft notice.

**Requirements.** herdr **0.9.1+**: that release is the first to route `file://` clicks to plugin
link handlers (older herdr ignores them, so the entry is inert there). The link must be one herdr
detects as a URL — an **OSC 8 hyperlink**, which is what most tools emit. The action is a bash
launcher for Linux, macOS, and WSL; on native Windows preview the click does nothing yet. No
keybinding is involved: the handler is a user gesture, not an event hook, so the viewer still only
ever appears when you ask for it.

## Limitation over `herdr --remote`

`--remote` attaches with **local** keybindings by default, but herdr does not send local custom
command bindings, including `plugin_action`, to the remote host. To drive the viewer on the remote,
put the binding in the remote server's `config.toml` and attach with
**`herdr --remote <host> --remote-keybindings server`**. The qualified id then resolves against the
plugin installed on that server.

This is a herdr keybinding/remote limitation, not the plugin's. The action and launcher work the
same locally and remotely; only which config supplies the binding differs.

On Windows the action ids and keybinding requirements differ slightly — see [Windows](windows.md).
