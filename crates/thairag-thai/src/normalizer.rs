/// Map legacy Thai-PDF Private-Use-Area glyph codepoints (U+F700–U+F713) back to
/// proper Thai combining marks. Many Thai PDFs — especially government documents —
/// embed fonts that place tone marks and upper vowels at PUA codepoints for glyph
/// positioning; text extractors emit those raw, which garbles the text, breaks
/// search/matching, and inflates embedding tokens (PUA byte-fallback-tokenizes at
/// up to 3 tokens/char). The PUA block holds positional *variants* of the same
/// marks (three parallel 5-mark groups), all of which normalize to one Unicode
/// character — glyph position is a font concern, not a text one.
///
/// Verified against real Thai tax-form PDFs: U+F70A→◌่, U+F70B→◌้, U+F70E→◌์,
/// U+F710→◌ิ, U+F712→◌ึ all reconstruct correct words (นำส่ง, เงินได้, ประโยชน์, สิทธิ).
/// 1:1 char replacement — preserves length, whitespace, and table layout.
pub fn map_thai_pua(text: &str) -> String {
    if !text.chars().any(|c| ('\u{F700}'..='\u{F713}').contains(&c)) {
        return text.to_string();
    }
    text.chars()
        .map(|c| match c as u32 {
            // tone marks — three positional variant groups → ่ ้ ๊ ๋ ์
            0xF700 | 0xF705 | 0xF70A => '\u{0E48}', // mai ek       ◌่
            0xF701 | 0xF706 | 0xF70B => '\u{0E49}', // mai tho      ◌้
            0xF702 | 0xF707 | 0xF70C => '\u{0E4A}', // mai tri      ◌๊
            0xF703 | 0xF708 | 0xF70D => '\u{0E4B}', // mai chattawa ◌๋
            0xF704 | 0xF709 | 0xF70E => '\u{0E4C}', // thanthakhat  ◌์
            0xF70F => '\u{0E4D}',                   // nikhahit     ◌ํ
            0xF710 => '\u{0E34}',                   // sara i       ◌ิ
            0xF711 => '\u{0E35}',                   // sara ii      ◌ี
            0xF712 => '\u{0E36}',                   // sara ue      ◌ึ
            0xF713 => '\u{0E37}',                   // sara uee     ◌ื
            _ => c,
        })
        .collect()
}

/// Recompose the decomposed Thai SARA AM that legacy PDFs emit. The character
/// ◌ำ (U+0E33) is often split for glyph layout into nikhahit + sara aa
/// (U+0E4D U+0E32); extractors emit the split form, so "นำส่ง" comes out as
/// "นํา..." and no longer matches normal Thai input or search queries. This
/// fuses the adjacent pair back into U+0E33. A preceding tone mark sits before
/// the nikhahit, so it is preserved (e.g. น + ◌้ + ◌ํ + า → น + ◌้ + ◌ำ).
pub fn recompose_sara_am(text: &str) -> String {
    if !text.contains('\u{0E4D}') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\u{0E4D}' && chars.get(i + 1) == Some(&'\u{0E32}') {
            out.push('\u{0E33}'); // ◌ำ
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Thai text normalizer.
/// Current implementation: basic whitespace normalization (stub).
pub struct ThaiNormalizer;

impl ThaiNormalizer {
    pub fn new() -> Self {
        Self
    }

    /// Normalize Thai text: collapse whitespace, trim.
    pub fn normalize(&self, text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

impl Default for ThaiNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_multiple_spaces() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("hello   world"), "hello world");
    }

    #[test]
    fn trim_leading_trailing() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("  hello  "), "hello");
    }

    #[test]
    fn mixed_whitespace() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("hello\t\nworld"), "hello world");
    }

    #[test]
    fn empty_string() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize(""), "");
    }

    #[test]
    fn whitespace_only() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("   \t  \n  "), "");
    }

    #[test]
    fn thai_text_preserved() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("สวัสดี  ครับ"), "สวัสดี ครับ");
    }

    #[test]
    fn single_word_unchanged() {
        let n = ThaiNormalizer::new();
        assert_eq!(n.normalize("hello"), "hello");
    }

    #[test]
    fn pua_maps_to_correct_thai_words() {
        // Real fragments observed in Thai tax-form PDFs (U+F70x glyph encoding).
        assert_eq!(map_thai_pua("นําส\u{F70A}ง"), "นําส\u{0E48}ง"); // นำส่ง: F70A→◌่
        assert_eq!(map_thai_pua("เงินได\u{F70B}"), "เงินได\u{0E49}"); // ได้: F70B→◌้
        assert_eq!(map_thai_pua("ประโยชน\u{F70E}"), "ประโยชน\u{0E4C}"); // ์: F70E→thanthakhat
        assert_eq!(map_thai_pua("ส\u{F710}ทธ\u{F710}"), "ส\u{0E34}ทธ\u{0E34}"); // สิทธิ: F710→◌ิ
        assert_eq!(map_thai_pua("ซ\u{F712}ง"), "ซ\u{0E36}ง"); // ◌ึ: F712
    }

    #[test]
    fn pua_map_is_length_preserving_and_layout_safe() {
        let s = "| a\u{F70B} | b\u{F70A} |\n| c | d |";
        let out = map_thai_pua(s);
        assert_eq!(out.chars().count(), s.chars().count()); // 1:1
        assert!(out.contains("|") && out.contains("\n")); // table layout untouched
        assert!(!out.chars().any(|c| ('\u{F700}'..='\u{F713}').contains(&c)));
    }

    #[test]
    fn pua_map_noop_on_clean_text() {
        assert_eq!(map_thai_pua("สวัสดีครับ ABC 123"), "สวัสดีครับ ABC 123");
    }

    #[test]
    fn recompose_sara_am_fuses_nikhahit_saraa() {
        // นํา (น + ◌ํ + า)  →  นำ (น + ◌ำ)
        assert_eq!(
            recompose_sara_am("น\u{0E4D}\u{0E32}ส\u{0E48}ง"),
            "น\u{0E33}ส\u{0E48}ง"
        );
        // กํา → กำ
        assert_eq!(recompose_sara_am("ก\u{0E4D}\u{0E32}หนด"), "ก\u{0E33}หนด");
        // tone mark before nikhahit is preserved: น + ◌้ + ◌ํ + า → น + ◌้ + ◌ำ
        assert_eq!(
            recompose_sara_am("น\u{0E49}\u{0E4D}\u{0E32}"),
            "น\u{0E49}\u{0E33}"
        );
    }

    #[test]
    fn recompose_sara_am_noop_when_no_nikhahit() {
        assert_eq!(recompose_sara_am("นำส่ง สวัสดี"), "นำส่ง สวัสดี");
    }
}
