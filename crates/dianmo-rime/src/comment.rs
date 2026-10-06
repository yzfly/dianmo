//! Candidate comments as the keyboard shows them (pure, unit-tested on any host).

/// rime-ice's `rime_ice` schema wraps spelling hints in `［…］` for corrector.lua; a correction
/// hint replaces it, otherwise corrector.lua clears it. Brackets that survive are stripped here.
/// With `hide_spelling` (T9, where the preedit already shows the pinyin) a plain spelling
/// comment is dropped.
pub fn display(raw: &str, hide_spelling: bool) -> Option<String> {
    let s = raw.trim();
    let s = s.strip_prefix('［').and_then(|s| s.strip_suffix('］')).unwrap_or(s).trim();
    if s.is_empty() || (hide_spelling && is_spelling(s)) {
        return None;
    }
    Some(s.to_string())
}

/// Lowercase syllables separated by spaces or apostrophes.
pub fn is_spelling(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == ' ' || c == '\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments() {
        assert_eq!(display("［ni hao］", false).as_deref(), Some("ni hao"));
        assert_eq!(display("ni hao", true), None);
        assert_eq!(display("", false), None);
        assert_eq!(display("［］", false), None);
        assert_eq!(display(" 〔x〕 ", true).as_deref(), Some("〔x〕"));
    }
}
