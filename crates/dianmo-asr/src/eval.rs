//! Character error rate for evaluating models against reference sentences.

/// Text used for CER: punctuation and spaces removed, ASCII letters upper-cased, full-width
/// letters/digits folded to ASCII. Each remaining char is one unit (Chinese chars, letters, digits).
pub fn normalize(s: &str) -> Vec<char> {
    s.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Levenshtein distance over chars.
pub fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + (ca != cb) as usize)
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// (edits, reference length) after [`normalize`]; CER = edits / length.
pub fn cer_counts(reference: &str, hypothesis: &str) -> (usize, usize) {
    let r = normalize(reference);
    (edit_distance(&r, &normalize(hypothesis)), r.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(
            normalize("请把 PPT，发给我。ok？"),
            "请把PPT发给我OK".chars().collect::<Vec<_>>()
        );
        assert_eq!(normalize("ＯＫ１"), vec!['O', 'K', '1']);
    }

    #[test]
    fn distance() {
        assert_eq!(cer_counts("今天下午三点", "今天下午三点。"), (0, 6));
        assert_eq!(cer_counts("今天下午三点", "今天下五点"), (2, 6)); // 1 sub + 1 del
        assert_eq!(cer_counts("", "多"), (1, 0));
    }
}
