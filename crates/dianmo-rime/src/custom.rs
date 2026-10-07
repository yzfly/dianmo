//! User customization of the rime-ice schemas (pure logic, testable anywhere): double pinyin
//! schemes, fuzzy pinyin (模糊音) patches, and the stamp that ties a user build to its inputs.
//!
//! Fuzzy pinyin is a `<schema>.custom.yaml` in the user directory that inserts `derive` rules at
//! the start of `speller/algebra`, i.e. on the full-pinyin syllables of the dictionary, before
//! rime-ice's own rules. That position works for every schema: full pinyin (where rime-ice keeps
//! its commented-out fuzzy rules), the double pinyin schemes (whose `xform` rules turn full pinyin
//! into key codes afterwards) and T9 (whose `derive/[abc]/2/`… rules turn letters into digits
//! afterwards).

/// Double pinyin (双拼) schemes; all are rime-ice schemas on the shared `rime_ice` dictionary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShuangpinScheme {
    /// 小鹤双拼 `double_pinyin_flypy`.
    #[default]
    Flypy,
    /// 自然码 `double_pinyin`.
    Ziranma,
    /// 微软双拼 `double_pinyin_mspy` (韵母 ing is the `;` key).
    Mspy,
    /// 搜狗双拼 `double_pinyin_sogou` (韵母 ing is the `;` key).
    Sogou,
}

impl ShuangpinScheme {
    pub const ALL: [ShuangpinScheme; 4] =
        [ShuangpinScheme::Flypy, ShuangpinScheme::Ziranma, ShuangpinScheme::Mspy, ShuangpinScheme::Sogou];

    /// The rime schema id.
    pub fn schema_id(self) -> &'static str {
        match self {
            ShuangpinScheme::Flypy => "double_pinyin_flypy",
            ShuangpinScheme::Ziranma => "double_pinyin",
            ShuangpinScheme::Mspy => "double_pinyin_mspy",
            ShuangpinScheme::Sogou => "double_pinyin_sogou",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ShuangpinScheme::Flypy => "小鹤双拼",
            ShuangpinScheme::Ziranma => "自然码",
            ShuangpinScheme::Mspy => "微软双拼",
            ShuangpinScheme::Sogou => "搜狗双拼",
        }
    }

    /// Whether the scheme types a key outside a–z (`;` for ing).
    pub fn uses_semicolon(self) -> bool {
        matches!(self, ShuangpinScheme::Mspy | ShuangpinScheme::Sogou)
    }
}

/// Fuzzy pinyin switches in this fixed order (same as dianmo-ui's `FuzzyPair::index`):
/// z/zh, c/ch, s/sh, n/l, an/ang, en/eng, in/ing.
pub type Fuzzy = [bool; 7];

pub const FUZZY_LABELS: [&str; 7] = ["z = zh", "c = ch", "s = sh", "n = l", "an = ang", "en = eng", "in = ing"];

/// Spelling algebra for each pair (both directions), applied to full-pinyin syllables.
const FUZZY_RULES: [[&str; 2]; 7] = [
    ["derive/^zh/z/", "derive/^z([^h])/zh$1/"],
    ["derive/^ch/c/", "derive/^c([^h])/ch$1/"],
    ["derive/^sh/s/", "derive/^s([^h])/sh$1/"],
    // not the bare syllables n / ng (嗯)
    ["derive/^n([aeiouv])/l$1/", "derive/^l([aeiouv])/n$1/"],
    // juan/quan/xuan/yuan have no -ang form; deriving one would collide with jiang/qiang/xiang
    // in double pinyin, where [iu]ang share a key.
    ["derive/(^|[^u]|[^jqxy]u)an$/$1ang/", "derive/ang$/an/"],
    ["derive/en$/eng/", "derive/eng$/en/"],
    ["derive/in$/ing/", "derive/ing$/in/"],
];

/// Schemas that get the fuzzy patch (everything the keyboard can select).
pub const FUZZY_SCHEMAS: [&str; 6] =
    ["rime_ice", "t9", "double_pinyin_flypy", "double_pinyin", "double_pinyin_mspy", "double_pinyin_sogou"];

/// The `derive` rules for `fuzzy`, in order.
pub fn fuzzy_rules(fuzzy: &Fuzzy) -> Vec<&'static str> {
    fuzzy.iter().zip(FUZZY_RULES.iter()).filter(|(on, _)| **on).flat_map(|(_, r)| r.iter().copied()).collect()
}

/// First line of every file Dianmo writes (so it is recognisable in the user directory).
pub const CUSTOM_HEADER: &str = "# 点墨 Dianmo：模糊音设置（由设置界面生成，不要手动修改）";

/// `<schema>.custom.yaml` for `fuzzy`; `None` when every pair is off (no file needed).
///
/// librime applies patch keys in sorted order; `@before 00`, `@before 01`, … (zero-padded so the
/// order is numeric) insert the rules at the start of the list in this order.
pub fn custom_yaml(fuzzy: &Fuzzy) -> Option<String> {
    let rules = fuzzy_rules(fuzzy);
    if rules.is_empty() {
        return None;
    }
    let on: Vec<&str> = fuzzy.iter().zip(FUZZY_LABELS).filter(|(on, _)| **on).map(|(_, l)| l).collect();
    let mut s = format!("{CUSTOM_HEADER}\n# {}\npatch:\n", on.join(", "));
    for (i, r) in rules.iter().enumerate() {
        s.push_str(&format!("  \"speller/algebra/@before {i:02}\": '{r}'\n"));
    }
    Some(s)
}

/// Which pairs a file written by [`custom_yaml`] turns on (all off for anything else).
pub fn parse_custom_yaml(text: &str) -> Fuzzy {
    let mut out = [false; 7];
    if !text.starts_with(CUSTOM_HEADER) {
        return out;
    }
    for (i, pair) in FUZZY_RULES.iter().enumerate() {
        out[i] = pair.iter().all(|r| text.contains(&format!("'{r}'")));
    }
    out
}

/// FNV-1a 64: a stable fingerprint (not security relevant).
#[derive(Clone, Copy)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub fn add(&mut self, bytes: &[u8]) -> &mut Self {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // field separator, so ("ab","c") != ("a","bc")
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        self
    }

    pub fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

/// Identifies what a user build was made from: the shared precompiled set (file names and sizes;
/// a new rime-ice or Dianmo data release changes it) and the customization files' contents.
/// `shared` and `custom` must be sorted by name.
pub fn stamp(shared: &[(String, u64)], custom: &[(String, String)]) -> String {
    let mut h = Fnv::default();
    h.add(b"dianmo-user-build-1");
    for (name, size) in shared {
        h.add(name.as_bytes()).add(&size.to_le_bytes());
    }
    for (name, text) in custom {
        h.add(name.as_bytes()).add(text.as_bytes());
    }
    h.hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_ids() {
        let ids: Vec<_> = ShuangpinScheme::ALL.iter().map(|s| s.schema_id()).collect();
        assert_eq!(ids, ["double_pinyin_flypy", "double_pinyin", "double_pinyin_mspy", "double_pinyin_sogou"]);
        for s in ShuangpinScheme::ALL {
            assert!(FUZZY_SCHEMAS.contains(&s.schema_id()));
        }
        assert!(ShuangpinScheme::Sogou.uses_semicolon() && !ShuangpinScheme::Ziranma.uses_semicolon());
    }

    #[test]
    fn no_fuzzy_no_file() {
        assert_eq!(custom_yaml(&[false; 7]), None);
        assert!(fuzzy_rules(&[false; 7]).is_empty());
    }

    #[test]
    fn yaml_inserts_rules_in_order() {
        let f = [true, false, false, true, false, false, true];
        let y = custom_yaml(&f).unwrap();
        assert!(y.starts_with(CUSTOM_HEADER));
        let lines: Vec<&str> = y.lines().filter(|l| l.contains("@before")).collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], "  \"speller/algebra/@before 00\": 'derive/^zh/z/'");
        assert_eq!(lines[5], "  \"speller/algebra/@before 05\": 'derive/ing$/in/'");
        // zero-padded keys sort in insertion order (librime applies patch keys sorted)
        let mut sorted = lines.clone();
        sorted.sort();
        assert_eq!(sorted, lines);
        assert_eq!(parse_custom_yaml(&y), f);
    }

    #[test]
    fn all_pairs_more_than_ten_rules_still_sorted() {
        let y = custom_yaml(&[true; 7]).unwrap();
        let lines: Vec<&str> = y.lines().filter(|l| l.contains("@before")).collect();
        assert_eq!(lines.len(), 14);
        let mut sorted = lines.clone();
        sorted.sort();
        assert_eq!(sorted, lines);
        assert!(lines[13].contains("@before 13"));
        assert_eq!(parse_custom_yaml(&y), [true; 7]);
        assert_eq!(parse_custom_yaml("patch: {}"), [false; 7]);
    }

    #[test]
    fn rules_have_no_single_quotes() {
        // they are written as single-quoted YAML scalars
        for pair in FUZZY_RULES {
            for r in pair {
                assert!(!r.contains('\''), "{r}");
                assert!(r.starts_with("derive/") && r.ends_with('/'), "{r}");
            }
        }
    }

    #[test]
    fn stamp_changes_with_inputs() {
        let shared = vec![("a.bin".to_string(), 10u64), ("b.yaml".to_string(), 20)];
        let custom = vec![("rime_ice.custom.yaml".to_string(), "x".to_string())];
        let s = stamp(&shared, &custom);
        assert_eq!(s.len(), 16);
        assert_eq!(s, stamp(&shared, &custom));
        let mut shared2 = shared.clone();
        shared2[1].1 = 21;
        assert_ne!(s, stamp(&shared2, &custom));
        assert_ne!(s, stamp(&shared, &[("rime_ice.custom.yaml".to_string(), "y".to_string())]));
        assert_ne!(stamp(&[("ab".into(), 1)], &[]), stamp(&[("a".into(), 1)], &[]));
    }
}
