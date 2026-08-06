//! Fuzzy subsequence matching for the repo search box.
//!
//! Hand-rolled rather than pulled from a matcher crate on purpose: repo names
//! are short, the candidate list is tens of rows, and the scoring rules a repo
//! dashboard actually needs are few enough to state in one screen and test
//! directly. Every knob below is a named constant so the ranking can be
//! reasoned about rather than tuned by feel.

/// Score for each matched character, before bonuses.
const MATCH_SCORE: i32 = 8;

/// Bonus when a match directly follows the previous one, so a contiguous run
/// ("repo" in "repoz") outranks the same characters scattered across a name.
const CONSECUTIVE_BONUS: i32 = 10;

/// Bonus when a match lands at the start of a word (string start, or just
/// after a separator). This is what lets an initialism find a hyphenated name:
/// "wtp" hits three word starts in "wireless-test-platform".
const WORD_START_BONUS: i32 = 14;

/// Penalty per haystack character skipped to reach a match, so a tight match
/// near the front beats a loose one strung out across a long name.
const GAP_PENALTY: i32 = 1;

/// Penalty per haystack character left over after the final match, so that
/// between two names the query matches equally well, the one it covers more of
/// wins: "pinz" ranks `pinz` above `pinz-board-archive`. Charged only on the
/// trailing remainder, not on total length, since penalizing length outright
/// would sink long hyphenated names that an initialism matches perfectly.
const TRAILING_PENALTY: i32 = 1;

/// Characters that begin a new word for `WORD_START_BONUS`. Covers the shapes
/// repo names actually take (`kebab-case`, `snake_case`, path-ish names).
fn is_separator(c: char) -> bool {
    matches!(c, '-' | '_' | '.' | '/' | ' ')
}

/// Score `haystack` against `needle`, case-insensitively. Higher is better.
///
/// Returns None when `needle` is not a subsequence of `haystack`, which is the
/// caller's "filter this row out" signal. An empty needle matches everything
/// with score 0, so an empty query neither filters nor reorders.
pub fn score(needle: &str, haystack: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }

    let hay: Vec<char> = haystack.to_lowercase().chars().collect();
    let needle: Vec<char> = needle.to_lowercase().chars().collect();

    let mut total = 0i32;
    // Where the search for the next needle char resumes: one past the last
    // match, so each haystack character is consumed at most once.
    let mut cursor = 0usize;
    let mut matched_any = false;

    for nc in needle {
        let found = hay[cursor..].iter().position(|&hc| hc == nc)? + cursor;

        total += MATCH_SCORE;
        // `found == cursor` means nothing was skipped, i.e. this match sits
        // immediately after the previous one.
        if matched_any && found == cursor {
            total += CONSECUTIVE_BONUS;
        }
        if found == 0 || is_separator(hay[found - 1]) {
            total += WORD_START_BONUS;
        }
        total -= (found - cursor) as i32 * GAP_PENALTY;

        cursor = found + 1;
        matched_any = true;
    }

    // `cursor` now sits one past the final match, so the remainder is the tail
    // the query never reached.
    total -= (hay.len() - cursor) as i32 * TRAILING_PENALTY;

    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_needle_matches_everything_neutrally() {
        // Score 0 for every candidate, so an empty query leaves the caller's
        // own ordering untouched rather than shuffling it.
        assert_eq!(score("", "fleetz"), Some(0));
        assert_eq!(score("", ""), Some(0));
    }

    #[test]
    fn non_subsequence_does_not_match() {
        assert_eq!(score("zzz", "fleetz"), None);
        // Right characters, wrong order: subsequence matching is ordered.
        assert_eq!(score("ztl", "fleetz"), None);
        assert_eq!(score("anything", ""), None);
    }

    #[test]
    fn matching_is_case_insensitive_both_ways() {
        assert!(score("FLEETZ", "fleetz").is_some());
        assert!(score("fleetz", "FLEETZ").is_some());
        assert_eq!(score("Fle", "fleetz"), score("fLE", "FleETZ"));
    }

    #[test]
    fn scattered_subsequence_still_matches() {
        // The whole point of fuzzy: "flz" is not a substring of "fleetz".
        assert!(score("flz", "fleetz").is_some());
        assert!(!"fleetz".contains("flz"));
    }

    #[test]
    fn contiguous_beats_scattered() {
        let contiguous = score("fle", "fleetz").unwrap();
        let scattered = score("flz", "fleetz").unwrap();
        assert!(
            contiguous > scattered,
            "contiguous {contiguous} should beat scattered {scattered}"
        );
    }

    #[test]
    fn word_starts_beat_mid_word_hits() {
        // "wtp" lands on three word starts here...
        let initialism = score("wtp", "wireless-test-platform").unwrap();
        // ...and on none here, buried mid-word.
        let buried = score("wtp", "swtpxx").unwrap();
        assert!(
            initialism > buried,
            "initialism {initialism} should beat buried {buried}"
        );
    }

    #[test]
    fn earlier_match_beats_later_one() {
        // Same characters, but a leading match skips nothing.
        let front = score("re", "repoz").unwrap();
        let back = score("re", "xxxxxrepoz").unwrap();
        assert!(front > back, "front {front} should beat back {back}");
    }

    #[test]
    fn exact_name_outranks_a_longer_name_sharing_the_prefix() {
        // Both match on the same prefix, so only the trailing remainder can
        // separate them. Without that, the two tie and the list falls back to
        // assembly order, putting the exact hit wherever it happened to sit.
        let exact = score("pinz", "pinz").unwrap();
        let longer = score("pinz", "pinz-board").unwrap();
        assert!(exact > longer, "exact {exact} should beat longer {longer}");
    }

    #[test]
    fn a_long_name_matched_by_an_initialism_still_outranks_a_buried_hit() {
        // Guards the trailing penalty against overreach: it must not become a
        // general "long names lose" rule, which would sink exactly the
        // hyphenated names initialisms are for.
        let initialism = score("wtp", "wireless-test-platform").unwrap();
        let buried = score("wtp", "swtpxx").unwrap();
        assert!(initialism > buried);
    }

    #[test]
    fn separators_all_count_as_word_boundaries() {
        for name in ["a-b", "a_b", "a.b", "a/b", "a b"] {
            let at_boundary = score("ab", name).unwrap();
            let mid_word = score("ab", "xab").unwrap();
            assert!(
                at_boundary > mid_word,
                "{name}: boundary {at_boundary} should beat mid-word {mid_word}"
            );
        }
    }
}
