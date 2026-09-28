//! Executable coverage for the file:// link-handler hand-off.
//!
//! The pure URL parser has unit coverage in `src/link_target.rs`; these tests exercise the
//! production boundary: the built binary's `--link-target` query (flag, env, and context-JSON
//! sources; its quiet non-zero exit for a non-file URL) and, on unix, the real
//! `scripts/open-file-link.sh` launcher against a recording herdr stub, proving the clicked URL
//! ends up as ONE `--env HERDR_FILE_VIEWER_OPEN=<target>` argv value on a `plugin pane open`.

mod common;

use common::TempDir;
use std::process::{Command, Output};

fn link_target(args: &[&str], env: &[(&str, &str)], clear: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_herdr-file-viewer"));
    cmd.args(args);
    for key in clear {
        cmd.env_remove(key);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output()
        .expect("run the built viewer's --link-target query")
}

const LINK_ENV: &[&str] = &["HERDR_PLUGIN_CLICKED_URL", "HERDR_PLUGIN_CONTEXT_JSON"];

fn assert_prints(output: &Output, expected: &str) {
    assert!(
        output.status.success(),
        "link-target query failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{expected}\n")
    );
    assert!(
        output.stderr.is_empty(),
        "the query is a quiet launcher contract: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn binary_link_target_flag_value_prints_the_open_target() {
    let out = link_target(
        &["--link-target", "file:///work/src/app.rs:42"],
        &[],
        LINK_ENV,
    );
    assert_prints(&out, "/work/src/app.rs:42");
    let out = link_target(
        &["--link-target=file:///work/my%20repo/README.md#L10-L20"],
        &[],
        LINK_ENV,
    );
    assert_prints(&out, "/work/my repo/README.md:10-20");
}

#[test]
fn binary_bare_link_target_reads_herdr_env_then_context_json() {
    // The launcher runs a bare `--link-target`; herdr's HERDR_PLUGIN_CLICKED_URL is the source …
    let out = link_target(
        &["--link-target"],
        &[("HERDR_PLUGIN_CLICKED_URL", "file:///work/a.rs:7")],
        LINK_ENV,
    );
    assert_prints(&out, "/work/a.rs:7");
    // … falling back to the context JSON's `clicked_url` when only that is set.
    let out = link_target(
        &["--link-target"],
        &[(
            "HERDR_PLUGIN_CONTEXT_JSON",
            r#"{"invocation_source":"link_click","clicked_url":"file:///work/b.rs"}"#,
        )],
        LINK_ENV,
    );
    assert_prints(&out, "/work/b.rs");
    // The env var wins over the JSON when both are present.
    let out = link_target(
        &["--link-target"],
        &[
            ("HERDR_PLUGIN_CLICKED_URL", "file:///work/env.rs"),
            (
                "HERDR_PLUGIN_CONTEXT_JSON",
                r#"{"clicked_url":"file:///work/json.rs"}"#,
            ),
        ],
        LINK_ENV,
    );
    assert_prints(&out, "/work/env.rs");
}

#[test]
fn binary_link_target_is_quiet_and_non_zero_for_a_non_file_link() {
    for (args, env) in [
        (vec!["--link-target", "https://example.com/a.rs"], vec![]),
        (vec!["--link-target", "file://nas/share/a.rs"], vec![]),
        (vec!["--link-target"], vec![]), // nothing to convert at all
        (
            vec!["--link-target"],
            vec![("HERDR_PLUGIN_CLICKED_URL", "mailto:someone@example.com")],
        ),
    ] {
        let out = link_target(&args, &env, LINK_ENV);
        assert!(
            !out.status.success(),
            "a non-file link must exit non-zero: {args:?} {env:?}"
        );
        assert!(
            out.stdout.is_empty() && out.stderr.is_empty(),
            "a non-file link prints nothing so the launcher opens no pane: {args:?}"
        );
    }
}

#[cfg(unix)]
mod unix_launcher {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn executable(path: &Path) {
        let mut permissions = std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)
            .unwrap_or_else(|e| panic!("chmod {}: {e}", path.display()));
    }

    /// Run `scripts/open-file-link.sh` in the plugin's on-disk layout against a herdr stub that
    /// records every argv it receives, with `clicked` as the URL herdr hands the action.
    fn run_link_launcher(clicked: Option<&str>, config: Option<&str>) -> (Output, String) {
        let temp = TempDir::new();
        let plugin_root = temp.path().join("plugin");
        let scripts_dir = plugin_root.join("scripts");
        let release_dir = plugin_root.join("target/release");
        let config_dir = temp.path().join("plugin-config");
        std::fs::create_dir_all(&scripts_dir).expect("create scripts dir");
        std::fs::create_dir_all(&release_dir).expect("create release dir");
        std::fs::create_dir_all(&config_dir).expect("create plugin config dir");

        let launcher = scripts_dir.join("open-file-link.sh");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/open-file-link.sh"),
            &launcher,
        )
        .expect("copy the link launcher");
        executable(&launcher);

        let viewer = release_dir.join("herdr-file-viewer");
        std::fs::copy(env!("CARGO_BIN_EXE_herdr-file-viewer"), &viewer)
            .expect("copy built viewer into the launcher's expected layout");
        executable(&viewer);

        if let Some(contents) = config {
            std::fs::write(config_dir.join("config.toml"), contents).expect("write config");
        }

        let capture = temp.path().join("herdr-calls");
        let fake_herdr = temp.path().join("herdr-stub");
        std::fs::write(
            &fake_herdr,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$HERDR_CAPTURE"
case "$1 $2" in
  "plugin config-dir") printf '%s\n' "$HERDR_TEST_CONFIG_DIR" ;;
esac
exit 0
"#,
        )
        .expect("write herdr stub");
        executable(&fake_herdr);

        let mut command = Command::new(&launcher);
        command
            .env("HERDR_BIN_PATH", &fake_herdr)
            .env("HERDR_CAPTURE", &capture)
            .env("HERDR_TEST_CONFIG_DIR", &config_dir)
            .env_remove("HERDR_PLUGIN_CONFIG_DIR")
            .env_remove("HERDR_PLUGIN_CONTEXT_JSON")
            .env("XDG_CONFIG_HOME", temp.path().join("empty-xdg"))
            .env("HOME", temp.path().join("empty-home"));
        match clicked {
            Some(url) => command.env("HERDR_PLUGIN_CLICKED_URL", url),
            None => command.env_remove("HERDR_PLUGIN_CLICKED_URL"),
        };
        let output = command.output().expect("run the link launcher");
        let calls = std::fs::read_to_string(&capture).unwrap_or_default();
        (output, calls)
    }

    #[test]
    fn link_launcher_opens_a_split_with_the_target_as_one_env_value() {
        let (output, calls) = run_link_launcher(
            Some("file:///work/repo/src/app.rs:42"),
            Some("open_direction = \"down\"\n"),
        );
        assert!(
            output.status.success(),
            "launcher failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        // The exact argv the launcher hands herdr (pinned, per AGENTS.md's verified-surface rule):
        // the manifest pane, a split in the configured direction, focused, with the open target
        // riding the pane's env — and NEVER `--cwd` (#139).
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            [
                "plugin config-dir herdr-file-viewer",
                "plugin pane open --plugin herdr-file-viewer --entrypoint file-viewer --placement split --direction down --focus --env HERDR_FILE_VIEWER_OPEN=/work/repo/src/app.rs:42",
            ]
        );
    }

    #[test]
    fn link_launcher_defaults_to_right_without_config() {
        let (output, calls) = run_link_launcher(Some("file:///work/repo/README.md"), None);
        assert!(output.status.success());
        let open = calls
            .lines()
            .find(|line| line.starts_with("plugin pane open "))
            .unwrap_or_else(|| panic!("launcher never opened a pane:\n{calls}"));
        assert!(
            open.contains("--direction right")
                && open.ends_with("--env HERDR_FILE_VIEWER_OPEN=/work/repo/README.md"),
            "unexpected open argv: {open}"
        );
    }

    #[test]
    fn link_launcher_opens_nothing_for_a_non_file_link_or_no_link() {
        for clicked in [
            Some("https://example.com/x.rs"),
            Some("file://nas/x.rs"),
            None,
        ] {
            let (output, calls) = run_link_launcher(clicked, None);
            assert!(
                !output.status.success(),
                "no target → the launcher exits non-zero ({clicked:?})"
            );
            assert!(
                !calls.contains("plugin pane open"),
                "no target → no pane is opened ({clicked:?}):\n{calls}"
            );
        }
    }
}
