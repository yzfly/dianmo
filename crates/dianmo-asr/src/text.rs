//! Clean up raw model output for typing into an app.

/// - drop `<unk>` (small models emit it for English words they don't know);
/// - drop spaces next to CJK characters / full-width punctuation (`这份 PPT 发到` → `这份PPT发到`,
///   `开会， 记得` → `开会，记得`), keep spaces between Latin words (`the day after`);
/// - join spelled-out letters (`P P T` → `PPT`, `O K` → `OK`).
pub fn tidy(raw: &str) -> String {
    let s = raw.replace("<unk>", " ");
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            let prev = words[i - 1];
            let a = prev.chars().last().unwrap_or(' ');
            let b = w.chars().next().unwrap_or(' ');
            let letters = is_letter(prev) && is_letter(w);
            if !(wide(a) || wide(b) || letters) {
                out.push(' ');
            }
        }
        out.push_str(w);
    }
    out
}

fn is_letter(w: &str) -> bool {
    let mut c = w.chars();
    matches!((c.next(), c.next()), (Some(x), None) if x.is_ascii_alphabetic())
}

/// CJK ideographs, kana/hangul and full-width forms / CJK punctuation.
fn wide(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

#[cfg(test)]
mod tests {
    use super::tidy;

    #[test]
    fn cleans() {
        assert_eq!(
            tidy("请把这份 P P T 发到我的邮箱里， 谢谢"),
            "请把这份PPT发到我的邮箱里，谢谢"
        );
        assert_eq!(tidy("这个功能已经 O K 了"), "这个功能已经OK了");
        assert_eq!(
            tidy("昨天是 Monday， today is 礼拜二， the day after tomorrow 是"),
            "昨天是Monday，today is礼拜二，the day after tomorrow是"
        );
        assert_eq!(tidy("请把这份<unk> 发到"), "请把这份发到");
        assert_eq!(tidy("  对 我 做了 介绍 "), "对我做了介绍");
        assert_eq!(tidy("I am OK"), "I am OK");
    }
}
