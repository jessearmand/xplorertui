use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

/// Greedy word-wrap that respects terminal display width (CJK-aware).
///
/// Returns chunks each having `UnicodeWidthStr::width(chunk) <= max_cols`,
/// with one documented exception: if a single character is wider than
/// `max_cols` (e.g. a CJK glyph at 2 cols when `max_cols == 1`), it is
/// placed on its own line as best-effort — splitting a single grapheme is
/// not meaningful, and the renderer's `Paragraph` will clip the overflow.
///
/// Always returns at least one chunk (an empty string for empty input)
/// so callers can rely on `chunks[0]` for the first row.
///
/// Algorithm:
/// 1. Iterate `split_whitespace()` tokens (collapses internal whitespace
///    runs and trims leading/trailing; callers that keep line breaks wrap
///    one line at a time).
/// 2. For each token of width `<= max_cols`, try to append to the current
///    line with one separator space. If it would overflow, flush the
///    current line and start a new one with the token.
/// 3. For a token wider than `max_cols`, flush the current line first to
///    keep word order, then walk the token char-by-char, flushing every
///    time the next char would overflow. After the token, continue
///    packing subsequent tokens onto the trailing partial chunk so we
///    don't waste columns.
///
/// Per-char width uses `UnicodeWidthChar::width(c).unwrap_or(0)` so
/// combining marks (width `None`) and control chars contribute 0 — they
/// piggyback on the previous char's column without consuming budget,
/// matching how terminals actually render them.
pub fn wrap_to_width(text: &str, max_cols: usize) -> Vec<String> {
    if max_cols == 0 {
        // Defensive: caller saturates to 1, but never panic on a 0 budget.
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w: usize = 0;

    for token in text.split_whitespace() {
        let token_w = UnicodeWidthStr::width(token);

        if token_w <= max_cols {
            // Token fits on a line by itself — try to append to current.
            let sep = if current.is_empty() { 0 } else { 1 };
            if current_w + sep + token_w <= max_cols {
                if sep == 1 {
                    current.push(' ');
                }
                current.push_str(token);
                current_w += sep + token_w;
            } else {
                lines.push(std::mem::take(&mut current));
                current.push_str(token);
                current_w = token_w;
            }
        } else {
            // Token wider than `max_cols` — hard-break char by char.
            // Flush the current line first so word order stays intact.
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_w = 0;
            }
            for c in token.chars() {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if current_w + cw > max_cols && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_w = 0;
                }
                current.push(c);
                current_w += cw;
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
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

#[cfg(test)]
mod wrap_tests {
    use super::*;

    fn widths(chunks: &[String]) -> Vec<usize> {
        chunks
            .iter()
            .map(|s| UnicodeWidthStr::width(s.as_str()))
            .collect()
    }

    #[test]
    fn empty_input_returns_single_empty_chunk() {
        assert_eq!(wrap_to_width("", 10), vec![String::new()]);
    }

    #[test]
    fn whitespace_only_returns_single_empty_chunk() {
        assert_eq!(wrap_to_width("   \t \n  ", 10), vec![String::new()]);
    }

    #[test]
    fn fits_in_one_line() {
        assert_eq!(wrap_to_width("hello world", 20), vec!["hello world"]);
    }

    #[test]
    fn greedy_packs_words() {
        let out = wrap_to_width("the quick brown fox jumps", 10);
        // "the quick" (9) | "brown fox" (9) | "jumps" (5)
        assert_eq!(out, vec!["the quick", "brown fox", "jumps"]);
        for w in widths(&out) {
            assert!(w <= 10);
        }
    }

    #[test]
    fn long_word_hard_breaks() {
        let out = wrap_to_width("abcdefghijklmnop", 5);
        // 16 chars, 5 cols each → "abcde", "fghij", "klmno", "p"
        assert_eq!(out, vec!["abcde", "fghij", "klmno", "p"]);
        for w in widths(&out) {
            assert!(w <= 5);
        }
    }

    #[test]
    fn long_word_after_short_word_flushes_first() {
        let out = wrap_to_width("hi supercalifragilistic world", 10);
        // "hi" stays its own line; long word breaks; "world" tail-packs.
        assert_eq!(out, vec!["hi", "supercalif", "ragilistic", "world"]);
    }

    #[test]
    fn cjk_counts_double_width() {
        // 4 CJK chars = 8 cols; max_cols = 4 → 2 chars per chunk.
        let out = wrap_to_width("脳波再生", 4);
        assert_eq!(out, vec!["脳波", "再生"]);
        for w in widths(&out) {
            assert!(w <= 4);
        }
    }

    #[test]
    fn cjk_token_in_word_stream() {
        // ASCII + CJK token wider than budget → hard-breaks the CJK.
        let out = wrap_to_width("hi 脳波再生デモ end", 6);
        // "hi" (2) | "脳波再" (6) | "生デモ" (6) | "end" (3)
        assert_eq!(out, vec!["hi", "脳波再", "生デモ", "end"]);
        for w in widths(&out) {
            assert!(w <= 6);
        }
    }

    #[test]
    fn combining_marks_do_not_consume_budget() {
        // "e\u{0301}" renders as 'é' (1 col); width-zero combining mark.
        let out = wrap_to_width("e\u{0301}xample fits", 8);
        // Width: 7 ("éxample" 7 + " " 1 + "fits" 4 = 12 > 8) — wait, that's
        // "éxample" = 7 cols, " fits" needs 5 more → 12 total > 8, so wraps.
        assert_eq!(out, vec!["e\u{0301}xample", "fits"]);
    }

    #[test]
    fn single_char_wider_than_max_is_best_effort() {
        // max_cols = 1, but '脳' is 2 cols. Documented best-effort: char
        // gets its own line, width exceeds budget, no panic.
        let out = wrap_to_width("脳波", 1);
        assert_eq!(out, vec!["脳", "波"]);
    }

    #[test]
    fn zero_max_returns_single_empty_chunk() {
        // Defensive — caller saturates to 1, but we don't panic.
        assert_eq!(wrap_to_width("anything", 0), vec![String::new()]);
    }

    #[test]
    fn never_returns_empty_vec() {
        for max in [1usize, 2, 5, 10, 80] {
            for input in ["", " ", "a", "abc def", "脳波 hello"] {
                let out = wrap_to_width(input, max);
                assert!(!out.is_empty(), "empty for input={input:?} max={max}");
            }
        }
    }
}
