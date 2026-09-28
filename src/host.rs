//! Host Adapter — the herdr boundary: parse the injected launch context (AC-26).
//!
//! `HERDR_PLUGIN_CONTEXT_JSON` is parsed defensively — malformed or missing input degrades
//! to a minimal `{ cwd }` context, never a panic (AC-26).

use crate::context::LaunchContext;
use serde::Deserialize;
use std::path::PathBuf;

/// The shape of `HERDR_PLUGIN_CONTEXT_JSON`. Every field is optional so a partial or absent
/// object degrades gracefully rather than failing to parse; unknown fields are ignored.
#[derive(Deserialize, Default)]
struct RawContext {
    /// herdr 0.7.0 reports the invoking pane's directory as `focused_pane_cwd` and the
    /// workspace root as `workspace_cwd`; a plain `cwd` is accepted as a fallback. The viewer
    /// roots at the most specific of these so the tree shows the directory the user is in — not
    /// the plugin's own install dir, where the pane process is actually started (the pane
    /// command is a relative path, so herdr launches it from the plugin root).
    focused_pane_cwd: Option<String>,
    workspace_cwd: Option<String>,
    cwd: Option<String>,
    base_branch: Option<String>,
    workspace_id: Option<String>,
    /// Set by herdr on a link-handler invocation (`invocation_source = "link_click"`): the URL the
    /// user Ctrl-clicked. Read only by the `--link-target` query, never by the TUI launch path.
    clicked_url: Option<String>,
}

/// The clicked URL a herdr link-handler invocation carries in `HERDR_PLUGIN_CONTEXT_JSON`
/// (`clicked_url`), or `None` when the context is absent, malformed, or not a link click. Never
/// panics (AC-26). The env-var form (`HERDR_PLUGIN_CLICKED_URL`) is the launcher's first choice;
/// this is the fallback for a herdr that sets only the JSON.
pub fn clicked_url_from_env() -> Option<String> {
    let json = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok();
    parse_clicked_url(json.as_deref())
}

/// Pure parser behind [`clicked_url_from_env`]: the `clicked_url` field of the context JSON, with
/// an empty string treated as absent.
pub fn parse_clicked_url(json: Option<&str>) -> Option<String> {
    let raw: RawContext = json
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    raw.clicked_url.filter(|s| !s.trim().is_empty())
}

/// Build a `LaunchContext` from the process environment: the injected context JSON, falling
/// back to the process working directory. Never panics (AC-26).
pub fn from_env() -> LaunchContext {
    let json = std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok();
    let cwd = std::env::current_dir().unwrap_or_default();
    parse_context(json.as_deref(), cwd)
}

/// Pure parser behind [`from_env`] (testable without touching process env). Missing or
/// malformed JSON yields a minimal `{ cwd: fallback_cwd }` context (AC-26).
pub fn parse_context(json: Option<&str>, fallback_cwd: PathBuf) -> LaunchContext {
    let raw: RawContext = json
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    // Ignore empty-string fields (a malformed host value) so they fall through to the next
    // candidate / the process-cwd fallback rather than rooting at an empty path.
    let cwd = raw
        .focused_pane_cwd
        .filter(|s| !s.is_empty())
        .or(raw.workspace_cwd.filter(|s| !s.is_empty()))
        .or(raw.cwd.filter(|s| !s.is_empty()))
        .map(PathBuf::from)
        .unwrap_or(fallback_cwd);
    LaunchContext {
        cwd,
        base_branch: raw.base_branch,
        workspace_id: raw.workspace_id.filter(|s| !s.is_empty()),
    }
}
