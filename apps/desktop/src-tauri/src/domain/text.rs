//! Text that came from outside the shell, cleaned before it is published.

/// Strips control characters and truncates `text` so a misbehaving service, printer queue, or peer
/// cannot corrupt UI rendering or log lines. An over-long value ends with `…` so truncation is
/// visible instead of pretending the value ended there.
pub(crate) fn sanitize_text(text: &str, limit: usize) -> String {
    let mut cleaned = String::with_capacity(text.len().min(limit));
    let mut truncated = false;
    for character in text.chars() {
        if cleaned.len() + character.len_utf8() > limit {
            truncated = true;
            break;
        }
        if character.is_control() {
            cleaned.push(' ');
        } else {
            cleaned.push(character);
        }
    }
    let cleaned = cleaned.trim();
    if truncated {
        format!("{cleaned}…")
    } else {
        cleaned.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_become_spaces() {
        assert_eq!(
            sanitize_text("started\r\nlistening\tnow", 120),
            "started  listening now"
        );
    }

    #[test]
    fn long_text_is_truncated_and_marked() {
        let cleaned = sanitize_text(&"a".repeat(500), 120);
        assert_eq!(cleaned.chars().count(), 121);
        assert!(cleaned.ends_with('…'));
    }

    #[test]
    fn short_text_is_returned_trimmed() {
        assert_eq!(sanitize_text("  ready  ", 120), "ready");
        assert_eq!(sanitize_text("", 120), "");
    }

    #[test]
    fn the_limit_counts_bytes_not_characters() {
        // A multi-byte character that would cross the byte limit is dropped whole.
        let cleaned = sanitize_text("ééé", 4);
        assert_eq!(cleaned, "éé…");
    }
}
