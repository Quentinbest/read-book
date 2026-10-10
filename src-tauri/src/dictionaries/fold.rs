//! DX4: how a selection finds its headword. Exact first, then folded: compatibility
//! forms (NFKC, so full-width letters and NFD accents match), case, curly and straight
//! apostrophes, and the hyphen family. Then, only when neither hits, labelled English
//! suffix rules that offer a candidate only if the dictionary has it, and never strip
//! what is part of the word (“news”, “bus”, “data”).

use unicode_normalization::UnicodeNormalization;

/// The folded form stored beside each headword and looked up second.
pub fn fold(s: &str) -> String {
    let nfkc: String = s.trim().nfkc().collect();
    nfkc.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{02BC}' | '\u{FF07}' | '`' | '\u{00B4}' => '\'',
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2212}' => '-',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Candidate base forms of an English word form, most likely first. The caller keeps
/// only the ones the dictionary has. Words of four letters or fewer are left alone.
pub fn suffix_candidates(word: &str) -> Vec<String> {
    let w = fold(word);
    if w.chars().count() <= 4
        || !w
            .chars()
            .all(|c| c.is_alphabetic() || c == '\'' || c == '-')
    {
        return vec![];
    }
    let mut out: Vec<String> = vec![];
    let mut push = |s: String| {
        if s.chars().count() >= 2 && s != w && !out.contains(&s) {
            out.push(s)
        }
    };
    let stem = |n: usize| w[..w.len() - n].to_string();
    if let Some(base) = w.strip_suffix("'s") {
        push(base.to_string());
    }
    if w.ends_with("ies") {
        push(format!("{}y", stem(3)));
    }
    if w.ends_with("es")
        && ["ches", "shes", "sses", "xes", "zes", "oes"]
            .iter()
            .any(|e| w.ends_with(e))
    {
        push(stem(2));
    }
    // -s, but not a word that ends in ss, us or is (“bus”, “analysis”).
    if w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is") {
        push(stem(1));
    }
    if w.ends_with("ied") {
        push(format!("{}y", stem(3)));
    }
    if w.ends_with("ed") {
        push(stem(1)); // invalidated → invalidate
        push(stem(2)); // walked → walk
        let s = stem(2);
        let b = s.as_bytes();
        if b.len() >= 2 && b[b.len() - 1] == b[b.len() - 2] {
            push(s[..s.len() - 1].to_string()); // stopped → stop
        }
    }
    if w.ends_with("ing") {
        let s = stem(3);
        push(format!("{s}e")); // caching → cache
        push(s.clone()); // walking → walk
        let b = s.as_bytes();
        if b.len() >= 2 && b[b.len() - 1] == b[b.len() - 2] {
            push(s[..s.len() - 1].to_string()); // running → run
        }
    }
    if w.ends_with("er") || w.ends_with("est") {
        let n = if w.ends_with("er") { 2 } else { 3 };
        push(stem(n));
        push(format!("{}e", stem(n)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_the_variants_a_book_writes() {
        assert_eq!(fold("it’s"), fold("it's"));
        assert_eq!(fold("ＣＡＣＨＥ"), "cache");
        assert_eq!(fold("cafe\u{301}"), fold("caf\u{e9}"));
        assert_eq!(fold("cross‐reference"), "cross-reference");
        assert_eq!(fold("  Two   Words "), "two words");
    }

    #[test]
    fn suffix_rules_offer_bases_and_never_strip_the_word() {
        assert!(suffix_candidates("invalidates").contains(&"invalidate".to_string()));
        assert!(suffix_candidates("caches").contains(&"cache".to_string()));
        assert!(suffix_candidates("queries").contains(&"query".to_string()));
        assert!(suffix_candidates("invalidated").contains(&"invalidate".to_string()));
        assert!(suffix_candidates("caching").contains(&"cache".to_string()));
        assert!(suffix_candidates("stopped").contains(&"stop".to_string()));
        assert!(suffix_candidates("running").contains(&"run".to_string()));
        // Too short, or the s belongs to the word.
        assert!(suffix_candidates("news").is_empty());
        assert!(suffix_candidates("bus").is_empty());
        assert!(suffix_candidates("data").is_empty());
        assert!(!suffix_candidates("analysis").contains(&"analysi".to_string()));
        assert!(!suffix_candidates("corpus").contains(&"corpu".to_string()));
        assert!(suffix_candidates("3.14s").is_empty());
    }
}
