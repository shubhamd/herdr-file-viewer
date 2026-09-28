//! Link target — turn a clicked `file://` URL into a launch **open target** (`path[:line[-end]]`).
//!
//! herdr routes a modified click (Ctrl+click) on a URL matching the manifest's `[[link_handlers]]`
//! pattern to the plugin's `open-file-link` action, handing it the URL in `HERDR_PLUGIN_CLICKED_URL`
//! (and as `clicked_url` in `HERDR_PLUGIN_CONTEXT_JSON`). The launcher script asks the binary
//! (`--link-target`) to convert that URL into the same `path` / `path:line` / `path:start-end`
//! string `--open` already accepts, so ONE tested parser owns percent-decoding, the host check and
//! the line-anchor conventions instead of a shell one-liner. Pure: no I/O, no filesystem access —
//! whether the path exists is decided later by `apply_open_target`, under the viewed root.
//!
//! Accepted shapes (the path part is percent-decoded):
//! - `file:///abs/path` and `file://localhost/abs/path` (any other host is refused: a remote
//!   `file://host/…` cannot be opened locally and must never turn into a local path)
//! - `file:///C:/work/app.rs` (a Windows drive after the leading slash)
//! - a line anchor, in the conventions agents and editors emit: a fragment `#L42`, `#L42-L58`,
//!   `#42`, `#L42C5` (column ignored), a query `?line=42`, or a trailing `:42` / `:42:5`
//!   (`path:line:col`) suffix on the path

use crate::open_target::OpenTarget;

/// The env var herdr sets on a link-handler action with the clicked URL (verified against the
/// herdr 0.7+ plugin docs: "shell plugins can also read `HERDR_PLUGIN_CLICKED_URL`").
pub const CLICKED_URL_ENV: &str = "HERDR_PLUGIN_CLICKED_URL";

/// Convert a `file://` URL into an [`OpenTarget`], or `None` when it is not a local file URL.
///
/// `None` (never a guess) for: a non-`file` scheme, a non-local host, an empty path, a
/// percent-encoding that does not decode to UTF-8, or a decoded path carrying control characters
/// (a crafted link must not smuggle ESC/newline into an argv or a notice).
pub fn file_url_to_open_target(url: &str) -> Option<OpenTarget> {
    let url = url.trim();
    let rest = strip_file_scheme(url)?;
    // `[authority]/path[?query][#fragment]` — the fragment and query are split off BEFORE the
    // path is decoded, so an encoded `%23` / `%3F` inside a file name stays part of the name.
    let (rest, fragment) = match rest.split_once('#') {
        Some((r, f)) => (r, Some(f)),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((r, q)) => (r, Some(q)),
        None => (rest, None),
    };
    let (authority, raw_path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        return None;
    }
    if raw_path.is_empty() {
        return None;
    }
    let mut path = percent_decode(raw_path)?;
    if path.chars().any(char::is_control) {
        return None;
    }
    // `/C:/work/app.rs` → `C:/work/app.rs`: the RFC 8089 spelling of a Windows drive path.
    if is_slash_drive_prefixed(&path) {
        path.remove(0);
    }
    // A line anchor: a trailing `:line[:col]` suffix always comes OFF the path (it is never part of
    // a file name in this shape), then the fragment wins, then the query, then that suffix.
    let suffix = strip_trailing_line(&mut path);
    let anchor = fragment
        .and_then(parse_line_anchor)
        .or_else(|| query.and_then(parse_line_query))
        .or(suffix);
    if path.is_empty() {
        return None;
    }
    let (line, end_line) = match anchor {
        Some((start, Some(end))) if start != end => (Some(start), Some(end)),
        Some((start, _)) => (Some(start), None),
        None => (None, None),
    };
    Some(OpenTarget {
        path,
        line,
        end_line,
    })
}

/// Strip a case-insensitive `file://` scheme, returning what follows it.
fn strip_file_scheme(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    rest.strip_prefix("//")
}

/// Decode `%XX` escapes to bytes and require the result to be UTF-8. `+` is left alone: it is a
/// literal plus in a path, not a space (that is form encoding, not URL paths).
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `/C:/…` or `/c:\…`: a leading slash before a one-letter drive and a colon.
fn is_slash_drive_prefixed(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':'
}

/// Parse one anchor endpoint: `42`, `L42`, `l42`, `L42C5` → `42`. Requires ≥ 1 leading digit after
/// the optional `L`; anything else (a heading slug like `#usage`) is not a line.
fn parse_anchor_endpoint(s: &str) -> Option<usize> {
    let s = s.strip_prefix(['L', 'l']).unwrap_or(s);
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &s[digits.len()..];
    // Only a column suffix (`C5`, `c5`) may follow the digits.
    let column_only = rest.is_empty()
        || rest
            .strip_prefix(['C', 'c'])
            .is_some_and(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_digit()));
    if !column_only {
        return None;
    }
    digits.parse::<usize>().ok().filter(|&n| n >= 1)
}

/// A fragment line anchor: `L42`, `42`, `L42-L58`, `L42-58`, `L42C5`, or `line=42`.
fn parse_line_anchor(fragment: &str) -> Option<(usize, Option<usize>)> {
    let fragment = fragment
        .strip_prefix("line=")
        .or_else(|| fragment.strip_prefix("line-"))
        .unwrap_or(fragment);
    match fragment.split_once('-') {
        Some((a, b)) => {
            let start = parse_anchor_endpoint(a)?;
            let end = parse_anchor_endpoint(b)?;
            Some(order(start, end))
        }
        None => parse_anchor_endpoint(fragment).map(|n| (n, None)),
    }
}

/// A `line=42` (or `line=42-58`) query parameter among `&`-separated pairs.
fn parse_line_query(query: &str) -> Option<(usize, Option<usize>)> {
    query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("line="))
        .find_map(parse_line_anchor)
}

/// Strip a trailing `:line` or `:line:col` from `path` (the `path:line:col` convention compilers
/// and agents print), returning the line. Leaves the path untouched when the suffix is not
/// digits, so a Windows drive (`C:/x`) or an odd file name keeps its colon.
fn strip_trailing_line(path: &mut String) -> Option<(usize, Option<usize>)> {
    let (head, last) = path.rsplit_once(':')?;
    if head.is_empty() || !is_digits(last) {
        return None;
    }
    // `path:line:col` — when the segment before is also digits, that one is the line.
    if let Some((head2, mid)) = head.rsplit_once(':')
        && !head2.is_empty()
        && is_digits(mid)
    {
        let line = mid.parse::<usize>().ok().filter(|&n| n >= 1)?;
        *path = head2.to_string();
        return Some((line, None));
    }
    // `path:line-end` is not a compiler shape; `--open` covers it, but a URL span rarely does.
    let line = last.parse::<usize>().ok().filter(|&n| n >= 1)?;
    *path = head.to_string();
    Some((line, None))
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn order(a: usize, b: usize) -> (usize, Option<usize>) {
    if a == b {
        (a, None)
    } else if a < b {
        (a, Some(b))
    } else {
        (b, Some(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(url: &str) -> Option<String> {
        file_url_to_open_target(url).map(|t| t.display_ref())
    }

    #[test]
    fn plain_absolute_file_url() {
        assert_eq!(
            target("file:///work/src/app.rs"),
            Some("/work/src/app.rs".into())
        );
    }

    #[test]
    fn localhost_authority_is_local() {
        assert_eq!(
            target("file://localhost/work/src/app.rs"),
            Some("/work/src/app.rs".into())
        );
        assert_eq!(
            target("file://LOCALHOST/work/src/app.rs"),
            Some("/work/src/app.rs".into())
        );
    }

    #[test]
    fn remote_host_is_refused() {
        assert_eq!(target("file://nas/share/app.rs"), None);
        assert_eq!(target("file://example.com/etc/passwd"), None);
    }

    #[test]
    fn non_file_schemes_are_refused() {
        assert_eq!(target("https://github.com/a/b/blob/main/src/app.rs"), None);
        assert_eq!(target("vscode://file/work/app.rs"), None);
        assert_eq!(target("/work/app.rs"), None);
        assert_eq!(target(""), None);
        assert_eq!(target("file:"), None);
        assert_eq!(target("file://"), None);
        assert_eq!(target("file:relative/app.rs"), None);
    }

    #[test]
    fn scheme_is_case_insensitive() {
        assert_eq!(target("FILE:///work/app.rs"), Some("/work/app.rs".into()));
        assert_eq!(target("File:///work/app.rs"), Some("/work/app.rs".into()));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            target("  file:///work/app.rs \n"),
            Some("/work/app.rs".into())
        );
    }

    #[test]
    fn percent_encoded_path_is_decoded() {
        assert_eq!(
            target("file:///work/my%20project/caf%C3%A9.md"),
            Some("/work/my project/café.md".into())
        );
        // An encoded `#` / `?` stays part of the NAME: it was split off only if literal.
        assert_eq!(
            target("file:///work/a%23b%3Fc.txt"),
            Some("/work/a#b?c.txt".into())
        );
    }

    #[test]
    fn bad_percent_encoding_or_invalid_utf8_is_refused() {
        assert_eq!(target("file:///work/app%2.rs"), None);
        assert_eq!(target("file:///work/app%zz.rs"), None);
        assert_eq!(target("file:///work/%ff%fe.rs"), None);
    }

    #[test]
    fn control_characters_are_refused() {
        // A crafted link must never smuggle ESC or a newline into an argv / a notice.
        assert_eq!(target("file:///work/a%1b%5d52.rs"), None);
        assert_eq!(target("file:///work/a%0arm.rs"), None);
    }

    #[test]
    fn trailing_colon_line_suffix() {
        assert_eq!(
            target("file:///work/src/app.rs:42"),
            Some("/work/src/app.rs:42".into())
        );
    }

    #[test]
    fn trailing_line_and_column_suffix_keeps_the_line_drops_the_column() {
        assert_eq!(
            target("file:///work/src/app.rs:42:7"),
            Some("/work/src/app.rs:42".into())
        );
    }

    #[test]
    fn line_zero_suffix_stays_on_the_path() {
        // `:0` is not a line (lines are 1-based), so it is left for the open-target parser, which
        // treats it the same way.
        assert_eq!(
            target("file:///work/app.rs:0"),
            Some("/work/app.rs:0".into())
        );
    }

    #[test]
    fn github_style_fragment_anchors() {
        assert_eq!(
            target("file:///work/app.rs#L42"),
            Some("/work/app.rs:42".into())
        );
        assert_eq!(
            target("file:///work/app.rs#L42-L58"),
            Some("/work/app.rs:42-58".into())
        );
        assert_eq!(
            target("file:///work/app.rs#L42-58"),
            Some("/work/app.rs:42-58".into())
        );
        assert_eq!(
            target("file:///work/app.rs#42"),
            Some("/work/app.rs:42".into())
        );
        assert_eq!(
            target("file:///work/app.rs#L42C5"),
            Some("/work/app.rs:42".into())
        );
        assert_eq!(
            target("file:///work/app.rs#L58-L42"),
            Some("/work/app.rs:42-58".into()),
            "a descending range is normalized"
        );
        assert_eq!(
            target("file:///work/app.rs#L42-L42"),
            Some("/work/app.rs:42".into()),
            "a degenerate range is a single line"
        );
    }

    #[test]
    fn non_line_fragment_is_ignored() {
        // A heading slug is not a line anchor; the file still opens at the top.
        assert_eq!(
            target("file:///work/README.md#usage"),
            Some("/work/README.md".into())
        );
        assert_eq!(target("file:///work/app.rs#L"), Some("/work/app.rs".into()));
        assert_eq!(
            target("file:///work/app.rs#L0"),
            Some("/work/app.rs".into())
        );
    }

    #[test]
    fn line_query_parameter() {
        assert_eq!(
            target("file:///work/app.rs?line=42"),
            Some("/work/app.rs:42".into())
        );
        assert_eq!(
            target("file:///work/app.rs?foo=1&line=42-58"),
            Some("/work/app.rs:42-58".into())
        );
        assert_eq!(
            target("file:///work/app.rs?foo=1"),
            Some("/work/app.rs".into())
        );
    }

    #[test]
    fn fragment_wins_over_query_and_suffix() {
        assert_eq!(
            target("file:///work/app.rs:7?line=8#L9"),
            Some("/work/app.rs:9".into()),
            "a fragment anchor is authoritative; the path's own suffix is stripped, not kept"
        );
        assert_eq!(
            target("file:///work/app.rs:7?line=8"),
            Some("/work/app.rs:8".into()),
            "the query beats the path suffix"
        );
    }

    #[test]
    fn windows_drive_path_drops_the_leading_slash() {
        assert_eq!(
            target("file:///C:/work/app.rs"),
            Some("C:/work/app.rs".into())
        );
        assert_eq!(
            target("file:///C:/work/app.rs:42"),
            Some("C:/work/app.rs:42".into())
        );
        assert_eq!(
            target("file:///c%3A/work/app.rs"),
            Some("c:/work/app.rs".into())
        );
    }

    #[test]
    fn result_round_trips_through_the_open_target_parser() {
        for url in [
            "file:///work/src/app.rs",
            "file:///work/src/app.rs:42",
            "file:///work/src/app.rs#L10-L20",
        ] {
            let t = file_url_to_open_target(url).unwrap();
            let reparsed = crate::open_target::parse_open_target(&t.display_ref()).unwrap();
            assert_eq!(
                reparsed, t,
                "{url}: the printed target must parse back identically"
            );
        }
    }
}
