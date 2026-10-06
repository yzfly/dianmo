//! Pure helpers for the 九宫格 schema (no librime calls, unit-tested on any host).
//!
//! rime-ice's `t9` schema derives a digit spelling for every pinyin syllable (`hao` -> `426`)
//! while keeping the letter spelling, so the input may mix locked letter syllables and digit
//! runs (`ni'426`). Candidate comments carry the full pinyin (`spelling_hints`).

/// The keypad digit for a lowercase letter.
pub fn digit_of(c: char) -> Option<char> {
    Some(match c {
        'a'..='c' => '2',
        'd'..='f' => '3',
        'g'..='i' => '4',
        'j'..='l' => '5',
        'm'..='o' => '6',
        'p'..='s' => '7',
        't'..='v' => '8',
        'w'..='z' => '9',
        _ => return None,
    })
}

/// Letters on a keypad digit.
pub fn letters_of(d: char) -> &'static str {
    match d {
        '2' => "abc",
        '3' => "def",
        '4' => "ghi",
        '5' => "jkl",
        '6' => "mno",
        '7' => "pqrs",
        '8' => "tuv",
        '9' => "wxyz",
        _ => "",
    }
}

/// `"ni"` -> `"64"`; `None` if `spelling` is not all lowercase letters.
pub fn digits_of(spelling: &str) -> Option<String> {
    spelling.chars().map(digit_of).collect()
}

fn is_delimiter(c: char) -> bool {
    c == ' ' || c == '\''
}

/// Byte offset in `input` where the unconfirmed part starts, given the composition preedit
/// and its `sel_start` (byte offset of the active segment). Everything from `sel_start` on is
/// raw input (digits/letters plus delimiters librime inserted for display), so counting its
/// non-delimiter chars from the end of `input` finds the boundary.
pub fn unconfirmed_start(input: &str, preedit: &str, sel_start: usize) -> usize {
    let tail = preedit.get(sel_start..).unwrap_or("");
    let mut want = tail.chars().filter(|&c| !is_delimiter(c)).count();
    if want == 0 {
        return input.len();
    }
    for (i, c) in input.char_indices().rev() {
        if !is_delimiter(c) {
            want -= 1;
            if want == 0 {
                return i;
            }
        }
    }
    0
}

/// Within `input[start..]`, skips already locked letter syllables and delimiters and returns
/// the byte range of the next digit run.
pub fn digit_run(input: &str, start: usize) -> std::ops::Range<usize> {
    let s = &input[start.min(input.len())..];
    let skip = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let begin = start + skip;
    let len = input[begin..].find(|c: char| !c.is_ascii_digit()).unwrap_or(input.len() - begin);
    begin..begin + len
}

/// Spellings the leading digits of `digits` can stand for, for the left column: first
/// syllables of the candidate comments whose keypad digits are a prefix of `digits` (in
/// candidate order, i.e. by frequency), then the single letters of the first digit.
pub fn spellings<'a>(digits: &str, comments: impl IntoIterator<Item = &'a str>, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(first) = digits.chars().next() else { return out };
    for comment in comments {
        let Some(syllable) = comment.split(is_delimiter).find(|s| !s.is_empty()) else { continue };
        if syllable.len() < 2 || out.iter().any(|s| s == syllable) {
            continue;
        }
        if digits_of(syllable).is_some_and(|d| digits.starts_with(&d)) {
            out.push(syllable.to_string());
            if out.len() >= max {
                break;
            }
        }
    }
    for l in letters_of(first).chars() {
        out.push(l.to_string());
    }
    out
}

/// Display preedit for T9: the active span shows the default candidate's pinyin instead of
/// digits. `preedit[..sel_start]` (already selected text) and `preedit[sel_end..]` (input the
/// candidate doesn't cover) are kept.
pub fn display_preedit(preedit: &str, sel_start: usize, sel_end: usize, comment: Option<&str>) -> String {
    let (Some(head), Some(tail)) = (preedit.get(..sel_start), preedit.get(sel_end..)) else {
        return preedit.to_string();
    };
    match comment.map(str::trim).filter(|c| !c.is_empty() && c.chars().all(|c| c.is_ascii_lowercase() || is_delimiter(c))) {
        Some(pinyin) if sel_end > sel_start => {
            let mut s = String::with_capacity(preedit.len() + 8);
            s.push_str(head);
            s.push_str(pinyin);
            if !tail.is_empty() {
                s.push(' ');
                s.push_str(tail.trim_start());
            }
            s
        }
        _ => preedit.to_string(),
    }
}

/// What Enter commits in T9: the display preedit without the syllable separators.
pub fn raw_commit(display: &str) -> String {
    display.chars().filter(|&c| !is_delimiter(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits() {
        assert_eq!(digits_of("nihao").as_deref(), Some("64426"));
        assert_eq!(digits_of("wxyz").as_deref(), Some("9999"));
        assert_eq!(digits_of("ni3"), None);
    }

    #[test]
    fn unconfirmed() {
        // nothing selected yet: active segment is the whole input
        assert_eq!(unconfirmed_start("64426", "64 426", 0), 0);
        // first syllable selected ("X" stands for a selected character, 3 bytes in UTF-8)
        assert_eq!(unconfirmed_start("64426", "\u{4e00}426", 3), 2);
        // locked letters + separator in input
        assert_eq!(unconfirmed_start("ni'426", "ni 426", 0), 0);
        assert_eq!(unconfirmed_start("64426", "\u{4e00}\u{4e00}", 6), 5);
    }

    #[test]
    fn runs() {
        assert_eq!(digit_run("64426", 0), 0..5);
        assert_eq!(digit_run("ni'426", 0), 3..6);
        assert_eq!(digit_run("ni'426", 3), 3..6);
        assert_eq!(digit_run("ni", 0), 2..2);
    }

    #[test]
    fn spelling_column() {
        let comments = ["ni hao", "mi", "ni", "ng", "ming", "o"];
        let s = spellings("64426", comments, 10);
        assert_eq!(s, ["ni", "mi", "ng", "m", "n", "o"]);
        assert!(spellings("", comments, 10).is_empty());
    }

    #[test]
    fn preedit_display() {
        assert_eq!(display_preedit("64 426", 0, 6, Some("ni hao")), "ni hao");
        assert_eq!(display_preedit("64 426", 0, 2, Some("ni")), "ni 426");
        assert_eq!(display_preedit("\u{4e00}426", 3, 6, Some("hao")), "\u{4e00}hao");
        assert_eq!(display_preedit("64426", 0, 5, None), "64426");
        assert_eq!(raw_commit("\u{4e00}ni hao"), "\u{4e00}nihao");
    }
}
