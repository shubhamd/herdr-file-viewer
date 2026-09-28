#!/usr/bin/env bash
# Link-handler launcher for the file viewer — herdr runs the `open-file-link` action when the user
# Ctrl-clicks a `file://` link that matches the manifest's `[[link_handlers]]` pattern (an explicit
# user gesture, never an event hook: the viewer still appears only when asked). herdr hands the
# clicked URL to this process as HERDR_PLUGIN_CLICKED_URL (and as `clicked_url` inside
# HERDR_PLUGIN_CONTEXT_JSON); this script opens a fresh Files split on that file, at its line when
# the link names one.
#
# The URL → open-target conversion (percent-decoding, the local-host check, `#L42` / `:42`
# anchors) is computed in-process by the viewer binary (`herdr-file-viewer --link-target`, which
# reads the env itself), so it is unit-tested and never re-implemented in shell. The target is then
# passed as ONE `--env` argv value: it is data, never interpolated into command text. A URL that is
# not a local file link (a remote host, a bad encoding) yields no target, and this script exits
# without opening anything.
#
# The viewer roots at the FOCUSED herdr pane's cwd (resolved to its worktree top level) — the pane
# the link was clicked in — so an absolute `file:///…` path under that repository opens directly;
# a path outside it is refused by the viewer with a soft "outside tree root" notice. Never pass
# `--cwd` here (#139: herdr resolves the manifest's relative pane command against it).
#
# WHICH WAY the split goes is the `open_direction` config key, asked of the same binary
# (`--open-direction`). herdr injects HERDR_PLUGIN_CONFIG_DIR into a pane IT spawns from the
# manifest, NOT into this action's process, so the directory is resolved via `plugin config-dir`
# first — the same trap `scripts/open-file-viewer.sh` documents.
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
viewer_bin="$script_dir/../target/release/herdr-file-viewer"

[ -x "$viewer_bin" ] || exit 1

# The open target for the clicked URL, or nothing (→ exit quietly) when it is not a local file link.
target="$("$viewer_bin" --link-target 2>/dev/null || true)"
[ -n "$target" ] || exit 1

# The configured split direction, defaulting to `right` on ANY failure. The `case` re-validates what
# the binary printed rather than trusting it into an argv (option-injection posture).
open_direction() {
  local dir cfg
  dir="right"
  cfg="${HERDR_PLUGIN_CONFIG_DIR:-$("$herdr_bin" plugin config-dir herdr-file-viewer 2>/dev/null || true)}"
  case "$(HERDR_PLUGIN_CONFIG_DIR="$cfg" "$viewer_bin" --open-direction 2>/dev/null || true)" in
    down) dir="down" ;;
  esac
  printf '%s' "$dir"
}

exec "$herdr_bin" plugin pane open \
  --plugin herdr-file-viewer \
  --entrypoint file-viewer \
  --placement split \
  --direction "$(open_direction)" \
  --focus \
  --env "HERDR_FILE_VIEWER_OPEN=$target"
