use super::*;
use crate::app::tests::{names_for, people_app};

#[test]
fn parse_query_extracts_tag_and_text() {
    let parsed = parse_query("repo #dirty");
    assert_eq!(parsed.text, "repo");
    assert!(parsed.filters.dirty);
    assert!(!parsed.filters.behind);
    assert!(parsed.unknown_tags.is_empty());
}

#[test]
fn parse_query_bare_hash_is_inert() {
    // It used to activate all six filters, which AND'd to an empty list --
    // and since "#" is the first keystroke of every tag, the list blanked
    // on the way to typing any of them.
    let parsed = parse_query("#");
    assert_eq!(parsed.text, "");
    assert_eq!(parsed.filters, QuickFilters::default());
    assert!(parsed.unknown_tags.is_empty(), "incomplete, not wrong");
}

#[test]
fn parse_query_reports_an_unknown_tag_instead_of_swallowing_it() {
    let parsed = parse_query("#nonsense hello");
    assert_eq!(parsed.text, "hello", "still not treated as literal text");
    assert_eq!(parsed.filters, QuickFilters::default());
    assert_eq!(parsed.unknown_tags, vec!["nonsense".to_string()]);
}

/// A text of separators alone splits into no words, and zero words
/// must not match: "-" or "." against an owner or author matched every
/// row. A name that holds the separator still matches it as a name.
#[test]
fn a_separator_only_query_matches_no_owner_or_author() {
    for text in ["-", ".", "/", "_", "- .", " "] {
        assert_eq!(person_score(text, "Gaurgle"), None, "{text:?}");
        assert_eq!(person_score(text, "Andreas Roos"), None, "{text:?}");
    }
    let mut app = people_app();
    assert!(names_for(&mut app, ".").is_empty());
    let mut found = names_for(&mut app, "-");
    found.sort();
    assert_eq!(found, vec!["tools-kestrel", "tools-scratch", "tools-web"]);
}
