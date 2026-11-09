use crate::model::{CiStatus, RepoStatus};

/// One quick-filter a repo can be toggled against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Dirty,
    Behind,
    Ahead,
    CiFailed,
    HasPrs,
    HasStashes,
}

/// Combinable quick filters for the repo list (AND'd together when several are active).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuickFilters {
    pub dirty: bool,
    pub behind: bool,
    pub ahead: bool,
    pub ci_failed: bool,
    pub has_prs: bool,
    pub has_stashes: bool,
}

impl QuickFilters {
    pub fn matches(&self, repo: &RepoStatus) -> bool {
        (!self.dirty || repo.dirty > 0)
            && (!self.behind || repo.behind > 0)
            && (!self.ahead || repo.ahead > 0)
            && (!self.ci_failed || repo.ci_status == CiStatus::Failed)
            && (!self.has_prs || repo.pr_count > 0)
            && (!self.has_stashes || repo.stash_count > 0)
    }
}

/// Short names quick filters answer to as `#tag` tokens in the search bar,
/// prefix-matched case-insensitively (so `#d` alone resolves to Dirty) --
/// mirrors notez/todoz's own `#tag` filter syntax.
pub const FILTER_TAGS: [(FilterKind, &str); 6] = [
    (FilterKind::Dirty, "dirty"),
    (FilterKind::Behind, "behind"),
    (FilterKind::Ahead, "ahead"),
    (FilterKind::CiFailed, "ci"),
    (FilterKind::HasPrs, "prs"),
    (FilterKind::HasStashes, "stash"),
];

fn set_filter(filters: &mut QuickFilters, kind: FilterKind) {
    match kind {
        FilterKind::Dirty => filters.dirty = true,
        FilterKind::Behind => filters.behind = true,
        FilterKind::Ahead => filters.ahead = true,
        FilterKind::CiFailed => filters.ci_failed = true,
        FilterKind::HasPrs => filters.has_prs = true,
        FilterKind::HasStashes => filters.has_stashes = true,
    }
}

/// A search-bar query split into its three parts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// The free-text portion, matched against repo names, owners and
    /// authors (`text_match`).
    pub text: String,
    /// Quick filters activated by `#tag` words.
    pub filters: QuickFilters,
    /// `#tag` words that matched no known filter, kept so the strip can flag
    /// them instead of letting a typo silently narrow nothing.
    pub unknown_tags: Vec<String>,
}

/// Split a search-bar query into free text, quick filters, and unrecognized
/// tags.
///
/// A `#name` word is always consumed as filter syntax and never falls back to
/// literal text, so tags stay unambiguous; an unrecognized one is reported in
/// `unknown_tags` rather than dropped on the floor. Prefix-matched against
/// `FILTER_TAGS`, so `#d` alone resolves to Dirty.
///
/// A bare `#` is deliberately inert. It used to activate every filter, which
/// AND'd to an almost always empty list, and since `#` is the first keystroke
/// of every tag the list blanked and repopulated on each one.
///
/// Filters live in the same text as the search query rather than on number
/// keys, so they combine the same way and never occupy a keybinding a
/// panel-focus jump could use instead.
pub fn parse_query(query: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut text_words = Vec::new();

    for word in query.split_whitespace() {
        let Some(name) = word.strip_prefix('#') else {
            text_words.push(word);
            continue;
        };
        let name = name.to_lowercase();
        if name.is_empty() {
            continue;
        }
        let mut matched = false;
        for &(kind, tag) in &FILTER_TAGS {
            if tag.starts_with(&name) {
                set_filter(&mut parsed.filters, kind);
                matched = true;
            }
        }
        if !matched {
            parsed.unknown_tags.push(name);
        }
    }

    parsed.text = text_words.join(" ");
    parsed
}

/// Which field of a row a free-text search matched (FZ-56), declared in
/// rank order: a name hit outranks an owner or organisation hit, which
/// outranks a contributor hit, whatever the fuzzy scores say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchField {
    Name,
    Owner,
    /// The owner, when GitHub reports it as an organisation.
    Org,
    /// An author of the row's loaded commits, or of one of its open PRs.
    Author,
}

impl MatchField {
    /// The rank tier: lower sorts first. Owner and organisation share one,
    /// being the same login.
    pub(super) fn tier(self) -> u8 {
        match self {
            MatchField::Name => 0,
            MatchField::Owner | MatchField::Org => 1,
            MatchField::Author => 2,
        }
    }

    /// The dimmed word the ranked list draws after a non-name hit, so a
    /// match on a person does not read as a mistaken name match.
    pub fn mark(self) -> Option<&'static str> {
        match self {
            MatchField::Name => None,
            MatchField::Owner => Some("owner"),
            MatchField::Org => Some("org"),
            MatchField::Author => Some("author"),
        }
    }
}

/// Score `text` against an owner or author string, or None when it does
/// not match.
///
/// Stricter than the name match: every query word must start a word of the
/// field (`roos` and `and ro` find "Andreas Roos", `dr` does not). A loose
/// subsequence made nearly any two letters match a full name, so a short
/// name query listed every row its author ever touched. Word order is free,
/// so the phrase and the words one by one find the same rows; the fuzzy
/// score then orders the hits, phrase first.
fn person_score(text: &str, field: &str) -> Option<i32> {
    fn words(s: &str) -> impl Iterator<Item = &str> {
        s.split(|c: char| matches!(c, '-' | '_' | '.' | '/') || c.is_whitespace())
            .filter(|w| !w.is_empty())
    }
    let field_lower = field.to_lowercase();
    let text_lower = text.to_lowercase();
    let field_words: Vec<&str> = words(&field_lower).collect();
    let text_words: Vec<&str> = words(&text_lower).collect();
    // Separators alone give no words, and zero words would match anyone.
    if text_words.is_empty() {
        return None;
    }
    let mut by_word = 0;
    for word in text_words {
        if !field_words.iter().any(|w| w.starts_with(word)) {
            return None;
        }
        by_word += crate::fuzzy::score(word, field).unwrap_or(0);
    }
    Some(crate::fuzzy::score(text, field).unwrap_or(by_word))
}

/// How a row answers a free-text query: the best field it matched and that
/// field's score, or None when it matched nothing. The name is tried first
/// and wins outright, so a name search ranks exactly as it did before
/// owners and authors were searched.
pub fn text_match(text: &str, repo: &RepoStatus) -> Option<(MatchField, i32)> {
    if let Some(score) = crate::fuzzy::score(text, &repo.name) {
        return Some((MatchField::Name, score));
    }
    if let Some(owner) = repo.owner_display()
        && let Some(score) = person_score(text, &owner)
    {
        let field = if repo.repo_kind == crate::model::RepoKind::Org {
            MatchField::Org
        } else {
            MatchField::Owner
        };
        return Some((field, score));
    }
    repo.commit_authors()
        .iter()
        .map(String::as_str)
        .chain(repo.prs.iter().map(|pr| pr.author.as_str()))
        .filter_map(|author| person_score(text, author))
        .max()
        .map(|score| (MatchField::Author, score))
}

#[cfg(test)]
mod tests;
