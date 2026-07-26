use crate::error::{Result, TermKeyError};

pub fn parse_https_url(input: &str) -> Result<url::Url> {
    if input.chars().any(|ch| {
        matches!(ch as u32, 0x00..=0x1f | 0x7f..=0x9f) || matches!(ch, '`' | '|' | '<' | '>')
    }) || input.contains("$(")
    {
        return Err(invalid_url_error());
    }

    let parsed = url::Url::parse(input).map_err(|_| invalid_url_error())?;
    if parsed.scheme() != "https"
        || !parsed.has_host()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(invalid_url_error());
    }

    Ok(parsed)
}

pub fn sanitize_terminal_text(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !matches!(*ch as u32, 0x00..=0x1f | 0x7f..=0x9f))
        .collect()
}

pub fn format_terminal_hyperlink(text: &str, url: &str) -> String {
    let text = sanitize_terminal_text(text);
    let Ok(url) = parse_https_url(url) else {
        return text;
    };

    format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url.as_str(), text)
}

pub fn open_url(input: &str) -> Result<()> {
    open_url_with(input, webbrowser::open)
}

fn open_url_with<F>(input: &str, opener: F) -> Result<()>
where
    F: FnOnce(&str) -> std::io::Result<()>,
{
    let parsed = parse_https_url(input)?;
    opener(parsed.as_str()).map_err(|error| TermKeyError::ConfigError(error.to_string()))
}

pub fn is_web_url(input: &str) -> bool {
    parse_https_url(input).is_ok()
}

fn invalid_url_error() -> TermKeyError {
    TermKeyError::ConfigError("URL must be a valid HTTPS URL without user information".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_http_userinfo_controls_and_shell_metacharacters() {
        for invalid in [
            "http://example.com",
            "https://user@example.com",
            "https://user:password@example.com",
            "https://example.com/\u{001b}[31m",
            "https://example.com/\u{0085}",
            "https://example.com/`whoami`",
            "https://example.com/$(whoami)",
            "https://example.com/a|whoami",
            "https://example.com/a>output",
            "https://example.com/a<input",
            "not a URL",
        ] {
            assert!(
                parse_https_url(invalid).is_err(),
                "accepted invalid HTTPS URL"
            );
            assert!(!is_web_url(invalid), "classified invalid URL as web URL");
        }
    }

    #[test]
    fn https_query_parameters_with_ampersand_remain_valid() {
        let parsed = parse_https_url("https://example.com/search?a=1&b=2").unwrap();

        assert_eq!(parsed.as_str(), "https://example.com/search?a=1&b=2");
        assert!(is_web_url("https://example.com/search?a=1&b=2"));
    }

    #[test]
    fn terminal_hyperlink_removes_escape_sequences() {
        let rendered = format_terminal_hyperlink(
            "safe\u{001b}]8;;https://attacker.invalid\u{001b}\\label\u{009b}31m",
            "https://example.com/path",
        );

        assert_eq!(
            rendered,
            "\u{001b}]8;;https://example.com/path\u{001b}\\safe]8;;https://attacker.invalid\\label31m\u{001b}]8;;\u{001b}\\"
        );
    }

    #[test]
    fn invalid_hyperlink_target_returns_sanitized_plain_text() {
        let rendered = format_terminal_hyperlink(
            "safe\u{0007}\u{001b}]0;owned\u{0007}label",
            "https://example.com/`whoami`",
        );

        assert_eq!(rendered, "safe]0;ownedlabel");
    }

    #[test]
    fn display_text_removes_c0_and_c1_controls() {
        let input: String = (0u32..=0x1f)
            .chain(0x7f..=0x9f)
            .filter_map(char::from_u32)
            .chain("visible".chars())
            .collect();

        assert_eq!(sanitize_terminal_text(&input), "visible");
    }

    #[test]
    fn opener_receives_only_the_structurally_parsed_url() {
        let mut opened = None;

        open_url_with("https://example.com/a?one=1&two=2", |url| {
            opened = Some(url.to_string());
            Ok(())
        })
        .unwrap();

        assert_eq!(opened.as_deref(), Some("https://example.com/a?one=1&two=2"));

        let mut invoked = false;
        let error = open_url_with("https://example.com/$(whoami)", |_| {
            invoked = true;
            Ok(())
        })
        .unwrap_err();

        assert!(!invoked);
        assert!(!error.to_string().contains("$(whoami)"));
    }

    #[test]
    fn hyperlink_format_wraps_web_urls() {
        let rendered = format_terminal_hyperlink("https://example.com", "https://example.com");
        assert!(rendered.contains("]8;;https://example.com"));
        assert!(rendered.contains("https://example.com"));
    }

    #[test]
    fn hyperlink_format_leaves_non_web_urls_plain() {
        assert_eq!(
            format_terminal_hyperlink("example.com", "example.com"),
            "example.com"
        );
    }
}
