//! Making text safe to keep: one physical line, no control characters, no
//! home directory, no key material, and a bounded length.
//!
//! This is the second line of defence. The first is that transcripts,
//! prompts, clipboard contents and keys are never handed to diagnostics at
//! all; a sanitizer cannot tell a sentence of dictation from any other text.

/// What an API key, a bearer token or a user's home directory becomes.
pub const REDACTED: &str = "[redacted]";
pub const HOME: &str = "$HOME";

/// Longest run of key-like characters after `sk-` that is still left alone.
/// OpenAI keys are far longer; model names such as `sk-learn` are not.
const KEY_MIN_LENGTH: usize = 16;
const TOKEN_MIN_LENGTH: usize = 8;

/// One line of text, safe to write or copy, of at most `max_bytes` bytes.
pub fn line(text: &str, max_bytes: usize) -> String {
    let single = single_line(text);
    let homed = redact_homes(&single, &home_prefixes());
    let keyless = redact_secrets(&homed);
    truncate(&keyless, max_bytes)
}

/// Line breaks and tabs become spaces; every other control character, and the
/// invisible direction overrides that can make a log line read differently
/// from what it says, is dropped.
fn single_line(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\r' | '\n' | '\t' | '\u{2028}' | '\u{2029}' => Some(' '),
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => None,
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// The directories this user's files live under, as the operating system
/// names them. The generic patterns below catch the rest.
fn home_prefixes() -> Vec<String> {
    let mut homes = Vec::new();
    for variable in ["HOME", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(variable) {
            let value = value
                .to_string_lossy()
                .trim_end_matches(['/', '\\'])
                .to_string();
            // A home of `/` or `C:` would redact every path there is.
            if value.len() > 3 {
                homes.push(value.replace('\\', "/"));
                homes.push(value.replace('/', "\\"));
            }
        }
    }
    homes.sort_by_key(|home| std::cmp::Reverse(home.len()));
    homes.dedup();
    homes
}

pub(crate) fn redact_homes(text: &str, homes: &[String]) -> String {
    let mut result = text.to_string();
    for home in homes {
        result = replace_ascii_case_insensitive(&result, home, HOME);
    }
    // Other accounts' directories, and a home the environment did not name.
    // Drive-letter forms first, so `D:/Users/x` loses its drive letter too.
    for marker in [":\\Users\\", ":/Users/", "/home/", "/Users/"] {
        result = redact_user_segment(&result, marker);
    }
    result
}

fn replace_ascii_case_insensitive(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut position = 0;
    while let Some(found) = lower_text[position..].find(&lower_needle) {
        let start = position + found;
        let end = start + needle.len();
        // Only a whole path component: `/home/jo` must not eat `/home/joanna`.
        let boundary = text[end..]
            .chars()
            .next()
            .is_none_or(|next| !is_name_character(next));
        result.push_str(&text[position..start]);
        result.push_str(if boundary {
            replacement
        } else {
            &text[start..end]
        });
        position = end;
    }
    result.push_str(&text[position..]);
    result
}

/// `…/home/<name>` becomes `$HOME`, keeping whatever came after the name.
fn redact_user_segment(text: &str, marker: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut position = 0;
    while let Some(found) = text[position..].find(marker) {
        let start = position + found;
        let name_start = start + marker.len();
        let name_length = text[name_start..]
            .char_indices()
            .find(|(_, c)| !is_name_character(*c))
            .map_or(text.len() - name_start, |(index, _)| index);
        if name_length == 0 {
            result.push_str(&text[position..name_start]);
            position = name_start;
            continue;
        }
        // A Windows drive letter belongs to the redacted prefix as well.
        let prefix_start = if marker.starts_with(':') {
            text[..start]
                .char_indices()
                .next_back()
                .filter(|(_, c)| c.is_ascii_alphabetic())
                .map_or(start, |(index, _)| index)
        } else {
            start
        };
        result.push_str(&text[position..prefix_start]);
        result.push_str(HOME);
        position = name_start + name_length;
    }
    result.push_str(&text[position..]);
    result
}

fn is_name_character(c: char) -> bool {
    !matches!(
        c,
        '/' | '\\' | '"' | '\'' | '`' | ':' | ',' | ';' | ')' | ']' | '}' | '>'
    ) && !c.is_whitespace()
}

fn is_token_character(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '+' | '/' | '=')
}

/// OpenAI keys and anything handed over as a bearer token or an
/// authorization header.
pub(crate) fn redact_secrets(text: &str) -> String {
    let keyed = redact_after(text, "sk-", KEY_MIN_LENGTH, false);
    let bearer = redact_after(&keyed, "bearer ", TOKEN_MIN_LENGTH, true);
    redact_after(&bearer, "authorization:", 1, true)
}

/// Replaces the token following each `marker` (ASCII case-insensitive) when
/// it is at least `min_length` characters long. `keep_marker` leaves the
/// marker itself readable, so the line still says what was removed.
fn redact_after(text: &str, marker: &str, min_length: usize, keep_marker: bool) -> String {
    let lower = text.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut position = 0;
    while let Some(found) = lower[position..].find(marker) {
        let start = position + found;
        let mut token_start = start + marker.len();
        if keep_marker {
            token_start += text[token_start..]
                .char_indices()
                .find(|(_, c)| *c != ' ')
                .map_or(text.len() - token_start, |(index, _)| index);
        }
        let token_length = text[token_start..]
            .char_indices()
            .find(|(_, c)| !is_token_character(*c))
            .map_or(text.len() - token_start, |(index, _)| index);
        let already = text[token_start..].starts_with(REDACTED);
        if token_length < min_length || already {
            result.push_str(&text[position..token_start]);
            position = token_start;
            continue;
        }
        result.push_str(&text[position..if keep_marker { token_start } else { start }]);
        result.push_str(REDACTED);
        position = token_start + token_length;
    }
    result.push_str(&text[position..]);
    result
}

/// At most `max_bytes` bytes, cut on a character boundary and marked.
pub fn truncate(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    const MARK: &str = "…";
    let mut end = max_bytes.saturating_sub(MARK.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARK}", &text[..end])
}

/// A source path from a panic or a backtrace, without the build machine's
/// directories: `…/.cargo/registry/src/<index>/tokio-1.48.0/src/x.rs` keeps
/// its crate and file, and a home directory is redacted like anywhere else.
pub fn source_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    for marker in ["/.cargo/registry/src/", "/.cargo/git/checkouts/"] {
        if let Some(index) = normalized.find(marker) {
            let rest = &normalized[index + marker.len()..];
            // Skip the registry index directory itself.
            let rest = rest
                .split_once('/')
                .map_or(rest, |(_, crate_path)| crate_path);
            return format!("[cargo]/{rest}");
        }
    }
    if let Some(index) = normalized.find("/rustc/") {
        return format!("[rustc]{}", &normalized[index + "/rustc".len()..]);
    }
    if let Some(index) = normalized.find("/src-tauri/") {
        return normalized[index + 1..].to_string();
    }
    line(&normalized, 512)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_one_line_without_control_characters() {
        assert_eq!(
            single_line("one\ntwo\r\nthree\tfour\u{1b}[31m\u{0}\u{202e}five\u{2028}six"),
            "one two  three four[31mfive six"
        );
    }

    #[test]
    fn home_directories_are_redacted_on_every_platform() {
        let homes = vec!["/home/jonas".to_string(), "C:\\Users\\Jonas".to_string()];
        assert_eq!(
            redact_homes("/home/jonas/.local/share/x.log", &homes),
            "$HOME/.local/share/x.log"
        );
        assert_eq!(
            redact_homes("C:\\Users\\jonas\\AppData\\Local", &homes),
            "$HOME\\AppData\\Local"
        );
        // A different account, and one the environment does not name.
        assert_eq!(
            redact_homes("/home/joanna/notes and /Users/alice/Library", &homes),
            "$HOME/notes and $HOME/Library"
        );
        assert_eq!(
            redact_homes("D:/Users/bob/Desktop/out.md", &[]),
            "$HOME/Desktop/out.md"
        );
        // Redacting twice changes nothing.
        let once = redact_homes("/home/jonas/a", &homes);
        assert_eq!(redact_homes(&once, &homes), once);
    }

    #[test]
    fn keys_and_bearer_material_never_survive() {
        let canary = "sk-proj-CANARYcanary0123456789abcdefXYZ";
        let text = format!(
            "key {canary} header Authorization: Bearer abcdefghijklmnop0123 and bearer  zyxwvutsrq987"
        );
        let redacted = redact_secrets(&text);
        assert!(!redacted.contains("CANARY"), "{redacted}");
        assert!(!redacted.contains("abcdefghijklmnop0123"), "{redacted}");
        assert!(!redacted.contains("zyxwvutsrq987"), "{redacted}");
        assert!(redacted.contains("Authorization:"));
        // Short words that merely start like a key are left alone.
        assert_eq!(
            redact_secrets("sk-learn bearer of news"),
            "sk-learn bearer of news"
        );
        // And redaction is stable when applied again.
        assert_eq!(redact_secrets(&redacted), redacted);
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let text = "ä".repeat(10);
        let cut = truncate(&text, 8);
        assert!(cut.len() <= 8);
        assert!(cut.ends_with('…'));
        assert_eq!(truncate("short", 8), "short");
    }

    #[test]
    fn build_machine_paths_keep_only_what_identifies_the_code() {
        assert_eq!(
            source_path(
                "/home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.48.0/src/runtime/park.rs"
            ),
            "[cargo]/tokio-1.48.0/src/runtime/park.rs"
        );
        assert_eq!(
            source_path("/Users/builder/work/utterform/src-tauri/src/audio.rs"),
            "src-tauri/src/audio.rs"
        );
        assert_eq!(source_path("src/audio.rs"), "src/audio.rs");
        assert_eq!(
            source_path("/rustc/abc123/library/core/src/str/mod.rs"),
            "[rustc]/abc123/library/core/src/str/mod.rs"
        );
    }
}
