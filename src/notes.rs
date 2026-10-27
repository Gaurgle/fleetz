//! The `notes` view's data (FZ-53): one walk of the selected repo for its
//! `notez/` and `.notez/` directories and its markdown, and the tree pane 2
//! draws from it. Design and rejected alternatives: `docs/ideas.md`, "Repo
//! notes in pane 2".
//!
//! The walk is `std::fs::read_dir`, as discovery's is, and prunes the same
//! directories (`discovery::PRUNE_DIRS`), so `target/` and `node_modules/`
//! cost nothing. Hidden directories are skipped except `.notez`, which is
//! where the gitignored scratch notes live. Nothing is indexed or written:
//! the app holds the result in memory per repo for the session.

use std::path::{Path, PathBuf};

/// The name of a notes directory, committed (`notez`) or scratch (`.notez`).
const NOTEZ_DIRS: [&str; 2] = ["notez", ".notez"];

/// How much of one file the inspector reads. A notes directory can hold an
/// image or a PDF; the body never pulls a whole one into memory.
pub const BODY_CAP: usize = 256 * 1024;

/// One file the walk collected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteFile {
    /// The path relative to the repo root.
    pub rel: PathBuf,
    /// The `notez/` or `.notez/` directory it sits under, relative to the
    /// repo root, when it sits under one. Any file there counts; outside
    /// one, only markdown was collected.
    pub notez: Option<PathBuf>,
}

/// What one walk of a repo found: every file under a notes directory and
/// every markdown file, in one list sorted by path. The `m` toggle only
/// changes which of them are drawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    pub files: Vec<NoteFile>,
}

/// One row of the tree as pane 2 draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A section: a notes directory, or in the all-markdown tree a directory
    /// holding markdown, by its repo-relative path (`./` for the root).
    Section(String),
    /// A directory nested inside a section, `depth` levels in.
    Dir { depth: usize, name: String },
    /// A file, `depth` levels into its section; `file` indexes
    /// `Notes::files`.
    File {
        depth: usize,
        name: String,
        file: usize,
    },
}

/// Walk the repo at `root`. An unreadable root is the error; an unreadable
/// directory below it is skipped, as discovery skips one.
pub fn walk(root: &Path) -> std::io::Result<Notes> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(root)?;
    visit(root, entries, Path::new(""), None, &mut files);
    files.sort_by(|a: &NoteFile, b: &NoteFile| a.rel.cmp(&b.rel));
    Ok(Notes { files })
}

fn visit(
    root: &Path,
    entries: std::fs::ReadDir,
    rel_dir: &Path,
    notez: Option<&Path>,
    out: &mut Vec<NoteFile>,
) {
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let rel = rel_dir.join(name.as_ref());
        // A symlink is followed to a file and never to a directory, so a
        // link loop cannot trap the walk.
        let is_file = kind.is_file()
            || (kind.is_symlink() && std::fs::metadata(entry.path()).is_ok_and(|m| m.is_file()));
        if kind.is_dir() {
            let notes_dir = NOTEZ_DIRS.contains(&name.as_ref());
            let hidden = name.starts_with('.') && !notes_dir;
            if hidden || crate::discovery::PRUNE_DIRS.contains(&name.as_ref()) {
                continue;
            }
            // A repo nested inside this one has its own row, and its notes
            // are that row's.
            if root.join(&rel).join(".git").exists() {
                continue;
            }
            let Ok(sub) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            let inner = if notes_dir {
                Some(rel.as_path())
            } else {
                notez
            };
            visit(root, sub, &rel, inner, out);
        } else if is_file && !name.starts_with('.') {
            let markdown = Path::new(name.as_ref())
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"));
            if notez.is_some() || markdown {
                out.push(NoteFile {
                    rel,
                    notez: notez.map(Path::to_path_buf),
                });
            }
        }
    }
}

impl Notes {
    /// The tree drawn for `all_markdown`, in drawing order. Notes-only:
    /// every notes directory a section, its files beneath and nested
    /// directories indented. All markdown: the same, and every other
    /// markdown file under a section for its directory. Sections run
    /// shallowest first, then by path.
    pub fn rows(&self, all_markdown: bool) -> Vec<Row> {
        // (section dir, file index), the section being the notes directory
        // or, for loose markdown, the file's own directory.
        let mut placed: Vec<(PathBuf, usize)> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| all_markdown || f.notez.is_some())
            .map(|(i, f)| {
                let section = match &f.notez {
                    Some(dir) => dir.clone(),
                    None => f.rel.parent().map(Path::to_path_buf).unwrap_or_default(),
                };
                (section, i)
            })
            .collect();
        placed.sort_by(|(sa, ia), (sb, ib)| {
            let depth = |p: &Path| p.components().count();
            (depth(sa), sa, &self.files[*ia].rel).cmp(&(depth(sb), sb, &self.files[*ib].rel))
        });

        let mut rows = Vec::new();
        let mut section: Option<&Path> = None;
        let mut open_dirs: Vec<String> = Vec::new();
        for (dir, i) in &placed {
            if section != Some(dir.as_path()) {
                section = Some(dir.as_path());
                open_dirs.clear();
                rows.push(Row::Section(section_label(dir)));
            }
            let inside = self.files[*i]
                .rel
                .strip_prefix(dir)
                .unwrap_or(&self.files[*i].rel);
            let mut parts: Vec<String> = inside
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let name = parts.pop().unwrap_or_default();
            // Directories this file shares with the one before stay open;
            // the rest are drawn from where the two part.
            let shared = open_dirs
                .iter()
                .zip(&parts)
                .take_while(|(a, b)| a == b)
                .count();
            open_dirs.truncate(shared);
            for part in &parts[shared..] {
                rows.push(Row::Dir {
                    depth: open_dirs.len(),
                    name: part.clone(),
                });
                open_dirs.push(part.clone());
            }
            rows.push(Row::File {
                depth: parts.len(),
                name,
                file: *i,
            });
        }
        rows
    }

    /// The files the tree for `all_markdown` lists, in the order it draws
    /// them: what the cursor walks.
    pub fn listed(&self, all_markdown: bool) -> Vec<&NoteFile> {
        self.rows(all_markdown)
            .into_iter()
            .filter_map(|row| match row {
                Row::File { file, .. } => self.files.get(file),
                _ => None,
            })
            .collect()
    }
}

/// A section's heading: its repo-relative path with a trailing slash, and
/// `./` for the repo root.
fn section_label(dir: &Path) -> String {
    if dir.as_os_str().is_empty() {
        "./".to_string()
    } else {
        format!("{}/", dir.display())
    }
}

/// Read the first `BODY_CAP` bytes of `path` as text, for the inspector. A
/// cut body says so on its last line; a file that is not text is an error.
pub fn read_body(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(BODY_CAP as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let cut = bytes.len() > BODY_CAP;
    bytes.truncate(BODY_CAP);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // The cap can split a character; only the end may be incomplete.
        Err(e) if cut && e.utf8_error().error_len().is_none() => {
            let valid = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).map_err(|e| e.to_string())?
        }
        Err(_) => return Err("not a text file".to_string()),
    };
    Ok(match cut {
        true => format!("{text}\n… cut at {} KiB", BODY_CAP / 1024),
        false => text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fleetz_test_notes_{name}"));
        let _ = std::fs::remove_dir_all(&root);
        for f in files {
            let path = root.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("# {f}\n")).unwrap();
        }
        root
    }

    fn drawn(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Section(s) => s.clone(),
                Row::Dir { depth, name } => format!("{}{name}/", "  ".repeat(depth + 1)),
                Row::File { depth, name, .. } => format!("{}{name}", "  ".repeat(depth + 1)),
            })
            .collect()
    }

    /// The ticket's fixture: three notes directories at three depths,
    /// markdown under `docs/`, and a `target/` full of markdown. The default
    /// tree is the three sections and nothing else; the toggle adds `docs/`
    /// and the root's README, never `target/`.
    #[test]
    fn the_tree_lists_notez_sections_and_the_toggle_adds_markdown() {
        let root = fixture(
            "tree",
            &[
                "README.md",
                "notez/plan.md",
                "notez/sub/deep.md",
                "notez/todo.txt",
                ".notez/scratch.md",
                "crates/x/notez/x.md",
                "crates/x/src/lib.rs",
                "docs/guide.md",
                "docs/img.png",
                "target/doc/api.md",
                "node_modules/pkg/README.md",
                ".hidden/secret.md",
            ],
        );
        let notes = walk(&root).unwrap();

        assert_eq!(
            drawn(&notes.rows(false)),
            [
                ".notez/",
                "  scratch.md",
                "notez/",
                "  plan.md",
                "  sub/",
                "    deep.md",
                "  todo.txt",
                "crates/x/notez/",
                "  x.md",
            ],
        );
        assert_eq!(
            drawn(&notes.rows(true)),
            [
                "./",
                "  README.md",
                ".notez/",
                "  scratch.md",
                "docs/",
                "  guide.md",
                "notez/",
                "  plan.md",
                "  sub/",
                "    deep.md",
                "  todo.txt",
                "crates/x/notez/",
                "  x.md",
            ],
        );
        let listed: Vec<_> = notes.listed(false).iter().map(|f| f.rel.clone()).collect();
        assert_eq!(
            listed,
            [
                ".notez/scratch.md",
                "notez/plan.md",
                "notez/sub/deep.md",
                "notez/todo.txt",
                "crates/x/notez/x.md",
            ]
            .map(PathBuf::from),
            "the cursor walks the files in drawing order",
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A repo with no notes directory draws no rows, and a nested repo's
    /// notes are its own row's.
    #[test]
    fn no_notez_draws_nothing_and_a_nested_repo_is_not_walked() {
        let root = fixture(
            "none",
            &["src/main.rs", "inner/.git/HEAD", "inner/notez/a.md"],
        );
        let notes = walk(&root).unwrap();
        assert!(notes.rows(false).is_empty());
        assert!(notes.rows(true).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_root_is_the_error() {
        let root = std::env::temp_dir().join("fleetz_test_notes_absent");
        let _ = std::fs::remove_dir_all(&root);
        assert!(walk(&root).is_err());
    }

    /// The body is the file's text; a file that is not text, or not there,
    /// is an error to show in place; a long one is cut and says so.
    #[test]
    fn a_body_reads_text_and_reports_what_it_cannot() {
        let root = fixture("body", &["notez/a.md"]);
        assert_eq!(
            read_body(&root.join("notez/a.md")),
            Ok("# notez/a.md\n".into())
        );
        assert!(read_body(&root.join("notez/missing.md")).is_err());

        std::fs::write(
            root.join("notez/bin.png"),
            [0x89, b'P', b'N', b'G', 0xff, 0xfe],
        )
        .unwrap();
        assert_eq!(
            read_body(&root.join("notez/bin.png")),
            Err("not a text file".into())
        );

        // A three-byte character straddling the cap.
        let mut long = "a".repeat(BODY_CAP - 1);
        long.push('€');
        std::fs::write(root.join("notez/long.md"), &long).unwrap();
        let body = read_body(&root.join("notez/long.md")).unwrap();
        assert!(body.starts_with(&"a".repeat(BODY_CAP - 1)));
        assert!(
            body.ends_with("… cut at 256 KiB"),
            "{}",
            &body[body.len() - 30..]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
