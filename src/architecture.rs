//! The layer test (A6 in `docs/architecture.md` section 7, L1 in
//! `docs/module-split-design.md`): reads every `src/**/*.rs`, drops the
//! `#[cfg(test)]` items, and fails when production code breaks A1 (down
//! only), A2 (no terminal below the front ends) or A3 (print mode never
//! imports the TUI), or when an allowlist entry no longer occurs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Module to layer, bottom layer first. A module is the first path segment
/// under `src/`; `main.rs` is `main`.
const LAYERS: [(&str, &[&str]); 4] = [
    (
        "domain",
        &["model", "time", "graph", "contributions", "digest", "fuzzy"],
    ),
    (
        "sources",
        &[
            "git",
            "github",
            "ci",
            "cache",
            "config",
            "discovery",
            "notes",
            "clipboard",
            "window_state",
            "cli",
        ],
    ),
    ("engine", &["engine"]),
    ("front ends", &["main", "app", "ui", "report"]),
];

/// Today's violations. The list may only shrink: an entry that no longer
/// occurs fails the test, so the fix that removes an edge removes its entry.
const ALLOWED: [(&str, &str); 0] = [];

const TERMINAL_CRATES: [&str; 2] = ["ratatui", "crossterm"];
const TERMINAL_OWNERS: [&str; 3] = ["main", "app", "ui"];
const TUI: [&str; 3] = ["main", "app", "ui"];

fn layer(module: &str) -> Option<usize> {
    LAYERS
        .iter()
        .position(|(_, modules)| modules.contains(&module))
}

fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// `src` with comments and the contents of string and char literals turned
/// into spaces, newlines kept, so brace matching and path search see code only.
fn blank_noncode(src: &str) -> Vec<char> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::with_capacity(c.len());
    let blank = |out: &mut Vec<char>, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if ch == '/' && next == Some('/') {
            while i < c.len() && c[i] != '\n' {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '/' && next == Some('*') {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                } else {
                    blank(&mut out, c[i]);
                    i += 1;
                    continue;
                }
                out.extend([' ', ' ']);
                i += 2;
                if depth == 0 {
                    break;
                }
            }
        } else if let Some((open, hashes)) = raw_string_open(&c, i) {
            out.extend(&c[i..open]);
            i = open;
            while i < c.len() {
                if c[i] == '"' && (1..=hashes).all(|h| c.get(i + h) == Some(&'#')) {
                    out.extend(&c[i..=i + hashes]);
                    i += hashes + 1;
                    break;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '"' {
            out.push('"');
            i += 1;
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' && i + 1 < c.len() {
                    blank(&mut out, c[i]);
                    i += 1;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
            if i < c.len() {
                out.push('"');
                i += 1;
            }
        } else if ch == '\'' && next == Some('\\') {
            // An escaped char literal: skip the escaped char, then to the quote.
            out.push('\'');
            let mut j = i + 1;
            blank(&mut out, c[j]);
            j += 1;
            if j < c.len() {
                blank(&mut out, c[j]);
                j += 1;
            }
            while j < c.len() && c[j] != '\'' {
                blank(&mut out, c[j]);
                j += 1;
            }
            if j < c.len() {
                out.push('\'');
                j += 1;
            }
            i = j;
        } else if ch == '\'' && c.get(i + 2) == Some(&'\'') {
            out.extend(['\'', ' ', '\'']);
            i += 3;
        } else {
            // Code, or the quote of a lifetime.
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// At a raw string (`r"`, `r#"`, `br#"`) starting at `i`: the index just
/// past its opening quote and its count of `#`.
fn raw_string_open(c: &[char], i: usize) -> Option<(usize, usize)> {
    if i > 0 && is_ident(c[i - 1]) {
        return None;
    }
    let mut j = i;
    if c.get(j) == Some(&'b') {
        j += 1;
    }
    if c.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hashes = c[j..].iter().take_while(|&&ch| ch == '#').count();
    j += hashes;
    (c.get(j) == Some(&'"')).then_some((j + 1, hashes))
}

/// Production code: `code` (already blanked) with every `#[cfg(test)]` item
/// turned into spaces, plus the names of modules declared only under
/// `#[cfg(test)]` (`mod tests;`), whose files are test code too.
fn strip_test_items(code: &[char]) -> (Vec<char>, Vec<String>) {
    const ATTR: &str = "#[cfg(test)]";
    let attr: Vec<char> = ATTR.chars().collect();
    let mut out = code.to_vec();
    let mut test_mods = Vec::new();
    let mut i = 0;
    while i + attr.len() <= code.len() {
        if code[i..i + attr.len()] != attr[..] {
            i += 1;
            continue;
        }
        let start = i + attr.len();
        // The item runs to a `;` outside any bracket, to the `}` that closes
        // its body, or up to an unmatched closer (the item was the last one
        // in its block). Further attributes are bracketed and fall inside.
        let mut depth = 0usize;
        let mut end = start;
        while end < code.len() {
            match code[end] {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' if depth == 0 => break,
                '}' if depth == 1 => {
                    end += 1;
                    break;
                }
                '}' | ')' | ']' => depth -= 1,
                ';' if depth == 0 => {
                    end += 1;
                    break;
                }
                _ => {}
            }
            end += 1;
        }
        let item: String = code[start..end].iter().collect();
        if let Some(name) = declared_module(&item) {
            test_mods.push(name);
        }
        for ch in &mut out[i..end] {
            if *ch != '\n' {
                *ch = ' ';
            }
        }
        i = end;
    }
    (out, test_mods)
}

/// The name in `mod name;` (with or without a visibility), else `None`.
fn declared_module(item: &str) -> Option<String> {
    let words: Vec<&str> = item.split_whitespace().collect();
    let text = words.join(" ");
    let text = ["pub(crate) ", "pub(super) ", "pub "]
        .iter()
        .find_map(|vis| text.strip_prefix(vis))
        .unwrap_or(&text);
    let name = text.strip_prefix("mod ")?.strip_suffix(';')?.trim();
    name.chars().all(is_ident).then(|| name.to_string())
}

/// What production `code` refers to: crate modules (`crate::git::`,
/// `crate::{git, ..}`, `crate::git;`), the crate root (`crate::item`, counted
/// as `main`), the bare `<mod>::` paths in `bare` (the modules `main.rs`
/// declares), and the terminal crates.
fn references(code: &[char], bare: &BTreeSet<String>) -> BTreeSet<String> {
    let crate_ref = |name: &str| -> String {
        if layer(name).is_some() {
            name.to_string()
        } else {
            "main".to_string()
        }
    };
    let ident_at =
        |at: usize| -> String { code[at..].iter().take_while(|&&ch| is_ident(ch)).collect() };
    let path_follows = |at: usize| code.get(at) == Some(&':') && code.get(at + 1) == Some(&':');
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i < code.len() {
        if !is_ident(code[i]) || (i > 0 && is_ident(code[i - 1])) {
            i += 1;
            continue;
        }
        let word = ident_at(i);
        let end = i + word.chars().count();
        let after_path = i >= 1 && code[i - 1] == ':';
        if word == "crate" && path_follows(end) {
            let at = end + 2;
            if code.get(at) == Some(&'{') {
                found.extend(group_heads(code, at).iter().map(|h| crate_ref(h)));
            } else {
                let name = ident_at(at);
                if !name.is_empty() {
                    found.insert(crate_ref(&name));
                }
            }
        } else if (!after_path && bare.contains(&word) && path_follows(end))
            || TERMINAL_CRATES.contains(&word.as_str())
        {
            found.insert(word.clone());
        }
        i = end;
    }
    found
}

/// The first segment of each path in the group `{a::X, b, c::{..}}` opening
/// at `open`.
fn group_heads(code: &[char], open: usize) -> Vec<String> {
    let mut heads = Vec::new();
    let mut depth = 0;
    let mut at_start = true;
    let mut i = open;
    while i < code.len() {
        let ch = code[i];
        if depth == 1 && at_start && is_ident(ch) {
            let head: String = code[i..].iter().take_while(|&&c| is_ident(c)).collect();
            i += head.chars().count();
            heads.push(head);
            at_start = false;
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => at_start = true,
            _ if ch.is_whitespace() => {}
            _ if depth == 1 => at_start = false,
            _ => {}
        }
        i += 1;
    }
    heads
}

/// The module a file under `src/` belongs to: its first path segment.
fn module_of(rel: &Path) -> String {
    let first = rel.components().next().expect("a path under src");
    let first = first.as_os_str().to_string_lossy();
    first.strip_suffix(".rs").unwrap_or(&first).to_string()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("read src entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// A rule broken by an edge between two modules, or a module and a crate.
fn broken_rule(from: &str, to: &str) -> Option<&'static str> {
    if TERMINAL_CRATES.contains(&to) {
        return (!TERMINAL_OWNERS.contains(&from)).then_some("A2");
    }
    if layer(to)? > layer(from)? {
        return Some("A1");
    }
    (from == "report" && TUI.contains(&to)).then_some("A3")
}

type Edges = BTreeMap<(String, String), (&'static str, BTreeSet<String>)>;

/// Every edge in production code under `src` that breaks a rule, with the
/// rule and the files that make it.
fn measure(src: &Path) -> Edges {
    let mut paths = Vec::new();
    rust_files(src, &mut paths);
    let mut files = Vec::new();
    let mut test_only: Vec<PathBuf> = Vec::new();
    for path in paths {
        let rel = path.strip_prefix(src).expect("under src").to_path_buf();
        let text = std::fs::read_to_string(&path).expect("read source file");
        let (code, test_mods) = strip_test_items(&blank_noncode(&text));
        // `mod x;` in main.rs or a mod.rs reads its sibling; in foo.rs, foo/x.rs.
        let owner = rel.file_name().and_then(|n| n.to_str());
        let base = match owner {
            Some("main.rs" | "mod.rs" | "lib.rs") => rel.parent().unwrap_or(Path::new("")).into(),
            _ => rel.with_extension(""),
        };
        test_only.extend(test_mods.iter().map(|name| base.join(name)));
        files.push((rel, code));
    }
    let is_test_only = |rel: &Path| {
        test_only
            .iter()
            .any(|m| rel == m.with_extension("rs") || rel.starts_with(m))
    };
    files.retain(|(rel, _)| !is_test_only(rel));
    let main_mods: BTreeSet<String> = files
        .iter()
        .filter(|(rel, _)| rel == Path::new("main.rs"))
        .flat_map(|(_, code)| {
            let text: String = code.iter().collect();
            text.split(';')
                .filter_map(|stmt| declared_module(&format!("{stmt};")))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut edges = Edges::new();
    for (rel, code) in &files {
        let from = module_of(rel);
        assert!(
            layer(&from).is_some(),
            "module `{from}` (src/{}) has no layer in LAYERS",
            rel.display()
        );
        let bare = if from == "main" {
            main_mods.clone()
        } else {
            BTreeSet::new()
        };
        for to in references(code, &bare) {
            if to == from {
                continue;
            }
            if let Some(rule) = broken_rule(&from, &to) {
                edges
                    .entry((from.clone(), to))
                    .or_insert((rule, BTreeSet::new()))
                    .1
                    .insert(format!("src/{}", rel.display()));
            }
        }
    }
    edges
}

#[test]
fn layers_hold_but_for_the_allowlist() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let edges = measure(&src);
    let allowed: BTreeSet<(String, String)> = ALLOWED
        .iter()
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .collect();
    let mut problems = Vec::new();
    for ((from, to), (rule, files)) in &edges {
        if !allowed.contains(&(from.clone(), to.clone())) {
            let files: Vec<&str> = files.iter().map(String::as_str).collect();
            problems.push(format!(
                "{rule}: {from} -> {to} is not allowed ({})",
                files.join(", ")
            ));
        }
    }
    for (from, to) in &allowed {
        if !edges.contains_key(&(from.clone(), to.clone())) {
            problems.push(format!(
                "allowlist: {from} -> {to} no longer occurs; remove it from ALLOWED"
            ));
        }
    }
    assert!(problems.is_empty(), "layer rules:\n{}", problems.join("\n"));
}

#[test]
fn test_items_and_noncode_do_not_count() {
    let src = r##"
use crate::git::Probe;
// crate::app::Comment
/// [crate::app::Doc]
const S: &str = "crate::app::{ ; }";
const R: &str = r#"crate::ui::Raw"#;
const C: char = '{';
fn f<'a>(x: &'a str) -> &'a str { x }
#[cfg(test)]
use crate::app::Single;
#[cfg(test)]
fn helper() { let _ = "}"; crate::app::Fn(); }
#[cfg(test)]
pub(crate) mod parity;
#[cfg(test)]
mod tests {
    use crate::app::App;
    mod inner { use crate::ui::render; }
}
pub fn after() { crate::time::now(); }
"##;
    let (code, test_mods) = strip_test_items(&blank_noncode(src));
    let refs = references(&code, &BTreeSet::new());
    assert_eq!(
        refs,
        BTreeSet::from(["git".to_string(), "time".to_string()])
    );
    assert_eq!(test_mods, vec!["parity".to_string()]);
}

#[test]
fn main_counts_bare_paths_and_crate_root_items() {
    let src = "use app::App;\nuse std::time::Duration;\nfn f() { ui::render(); \
               tokio::time::sleep(); crate::{git::Probe, carries}; }\n";
    let bare = BTreeSet::from(["app".to_string(), "ui".to_string(), "time".to_string()]);
    let refs = references(&blank_noncode(src), &bare);
    let want = ["app", "ui", "git", "main"].map(String::from);
    assert_eq!(refs, BTreeSet::from(want));
}
