use unicode_width::UnicodeWidthChar;

const ELLIPSIS: char = '…';

/// Truncate `s` so its terminal display width is at most `max_cols` columns.
///
/// If truncation is required, the result ends with a single-column `…`
/// so the returned string's width stays within `max_cols`. Never panics
/// on any UTF-8 input, including CJK and emoji. Chars with undefined
/// width (control characters) contribute 0 columns.
pub fn truncate_for_width(s: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }

    let mut full_width = 0usize;
    for c in s.chars() {
        full_width = full_width.saturating_add(c.width().unwrap_or(0));
        if full_width > max_cols {
            break;
        }
    }
    if full_width <= max_cols {
        return s.to_string();
    }

    // Reserve one column for the ellipsis.
    let budget = max_cols - 1;
    let mut out = String::with_capacity(s.len().min(max_cols * 4));
    let mut acc = 0usize;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if acc + w > budget {
            break;
        }
        out.push(c);
        acc += w;
    }
    out.push(ELLIPSIS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn empty_max_returns_empty() {
        assert_eq!(truncate_for_width("hello", 0), "");
    }

    #[test]
    fn fits_ascii_unchanged() {
        assert_eq!(truncate_for_width("hello", 10), "hello");
        assert_eq!(truncate_for_width("hello", 5), "hello");
    }

    #[test]
    fn ascii_truncates_with_ellipsis() {
        let out = truncate_for_width("abcdefghij", 5);
        assert_eq!(out, "abcd…");
        assert_eq!(out.width(), 5);
    }

    #[test]
    fn cjk_counts_width_two() {
        // 「脳波」 is 2 CJK chars at width 2 each → 4 cols total.
        assert_eq!("脳波".width(), 4);
        // Fits exactly — no truncation, no ellipsis.
        assert_eq!(truncate_for_width("脳波", 4), "脳波");
        // Truncating "脳波再生" (8 cols) down to 4 cols: reserve 1 col for
        // `…`, leaving 3 cols of content budget — fits one CJK char (2 cols).
        let out = truncate_for_width("脳波再生", 4);
        assert_eq!(out, "脳…");
        assert!(out.width() <= 4);
    }

    #[test]
    fn cjk_truncates_cleanly() {
        // 5-col budget: 1 CJK char (2 cols) + `…` (1 col) = 3 cols used, or
        // 2 CJK chars (4 cols) + `…` (1 col) = 5 cols — the latter wins.
        let out = truncate_for_width("脳波再生デモ", 5);
        assert_eq!(out, "脳波…");
        assert!(out.width() <= 5);
    }

    #[test]
    fn does_not_panic_on_the_reported_crash_string() {
        // Exact substring pattern from the original panic report.
        let s = "RT @KHloCSIavYLAiKZ: TAB5の脳波再生デモ波形をWIFIで\
                 AtomS3Liteに送ってUSB-OTGでサーマルプリンタに\
                 データ送って印刷できるようにできた。";
        for max in [0, 1, 3, 7, 10, 40, 80, 140] {
            let out = truncate_for_width(s, max);
            assert!(out.width() <= max, "width {} > max {}", out.width(), max);
        }
    }

    #[test]
    fn max_one_returns_just_ellipsis_or_single_ascii() {
        // Single ASCII fits.
        assert_eq!(truncate_for_width("a", 1), "a");
        // With a longer string, only the ellipsis fits in 1 column.
        assert_eq!(truncate_for_width("abc", 1), "…");
        // A width-2 char cannot coexist with the ellipsis at max=1.
        assert_eq!(truncate_for_width("脳波", 1), "…");
    }

    #[test]
    fn handles_combining_characters_without_panic() {
        // Combining marks have width 0; the base char has its own width.
        let s = "e\u{0301}xample"; // 'é' as e + combining acute.
        let out = truncate_for_width(s, 4);
        assert!(out.width() <= 4);
    }
}
