use std::io::Read;

use herdr_file_viewer::open_target::{self, CliAction};

fn main() -> std::io::Result<()> {
    // Argv parsing lives in the library (`open_target::parse_args`) so it is unit-tested and so
    // unknown / bare flags degrade instead of killing a herdr-spawned pane.
    match open_target::parse_args(std::env::args().skip(1)) {
        CliAction::LaunchDecision => {
            let mut json = String::new();
            std::io::stdin().read_to_string(&mut json)?;
            println!("{}", herdr_file_viewer::launch::launch_decision(&json));
            Ok(())
        }
        CliAction::LaunchDecisionTab => {
            let mut json = String::new();
            std::io::stdin().read_to_string(&mut json)?;
            println!("{}", herdr_file_viewer::launch::launch_decision_tab(&json));
            Ok(())
        }
        CliAction::PrintOpenDirection => {
            // The launcher's one question: which way should herdr split? Resolving it here rather
            // than parsing TOML in bash/PowerShell keeps the lenient-value rules (trim, case,
            // `bottom`) in the one tested place. A missing or malformed config resolves to the
            // default `right`, so the launch is never blocked by a config problem.
            let (config, _) = herdr_file_viewer::config::load_config_from_env();
            let eff = herdr_file_viewer::config::resolve(&config, |k| std::env::var(k).ok());
            println!("{}", eff.open_direction.label());
            Ok(())
        }
        CliAction::LinkTarget { url } => {
            // The link-handler launcher's one question: which open target does this clicked
            // `file://` URL name? Resolved here rather than in bash so percent-decoding, the
            // local-host check and the `#L42` / `:42` anchor conventions live in the one tested
            // place. URL precedence: the flag value, then herdr's HERDR_PLUGIN_CLICKED_URL, then the
            // context JSON's `clicked_url`. A URL that is not a local file link prints nothing and
            // exits non-zero, so the launcher opens no pane for it.
            let url = url
                .or_else(|| {
                    std::env::var(herdr_file_viewer::link_target::CLICKED_URL_ENV)
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                })
                .or_else(herdr_file_viewer::host::clicked_url_from_env);
            match url
                .as_deref()
                .and_then(herdr_file_viewer::link_target::file_url_to_open_target)
            {
                Some(target) => {
                    println!("{}", target.display_ref());
                    Ok(())
                }
                None => std::process::exit(1),
            }
        }
        CliAction::Run { open } => herdr_file_viewer::run(open),
    }
}
