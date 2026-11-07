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

/// How much of one file the reader reads. A notes directory can hold an
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

/// What a note file is, from its name alone (FZ-62): the tree's glyph, the
/// card's and the reader's word. `Todo` is any `TODO.md`, which FZ-63 draws
/// as a board; until then it reads as the markdown it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Markdown,
    Todo,
    Text,
    Other,
}

impl NoteKind {
    /// The word the card and the reader's border say.
    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Todo => "todo",
            Self::Text => "text",
            Self::Other => "other",
        }
    }

    /// Whether the reader renders this kind as markdown (FZ-59): a `TODO.md`
    /// is markdown too; text and other files are always raw.
    pub fn is_markdown(self) -> bool {
        matches!(self, Self::Markdown | Self::Todo)
    }
}

impl NoteFile {
    /// Whether this is the personal board (`personal_board`), outside the
    /// repo: the one file whose `rel` is absolute.
    pub fn is_personal(&self) -> bool {
        self.rel.is_absolute()
    }

    /// Where a board sits, as the `todos` section writes it beside the
    /// name: its directory in the repo (`./` for the root), or `personal`.
    pub fn place(&self) -> String {
        match self.is_personal() {
            true => "personal".to_string(),
            false => section_label(self.rel.parent().unwrap_or(Path::new(""))),
        }
    }

    /// The file's kind, from its name and extension. A method rather than a
    /// field: the walk already holds the name, and nothing is stored twice.
    pub fn kind(&self) -> NoteKind {
        let name = self
            .rel
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.eq_ignore_ascii_case("TODO.md") {
            return NoteKind::Todo;
        }
        let ext = Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        match ext.as_deref() {
            Some("md" | "markdown") => NoteKind::Markdown,
            Some("txt") => NoteKind::Text,
            _ => NoteKind::Other,
        }
    }
}

/// What the card says about a file from its metadata (FZ-62): read off the
/// UI thread in the same blocking read as the body
/// (`main.rs::spawn_note_read`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteFacts {
    /// Bytes on disk; `None` when the metadata could not be read.
    pub size: Option<u64>,
    pub modified: Option<std::time::SystemTime>,
    /// `None` where the filesystem keeps no birth time.
    pub created: Option<std::time::SystemTime>,
}

/// The file's last commit, as git answers after the body has landed: git
/// walks the whole history for a file it never saw, so the body never waits
/// for this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LastCommit {
    /// git printed nothing: never committed, as every `.notez/` file is.
    Untracked,
    /// The commit date as git prints it (`%cI`).
    At(String),
}

/// The size, modified and created times of `path`, from one `metadata`
/// call.
pub fn file_facts(path: &Path) -> NoteFacts {
    let Ok(meta) = std::fs::metadata(path) else {
        return NoteFacts::default();
    };
    NoteFacts {
        size: Some(meta.len()),
        modified: meta.modified().ok(),
        created: meta.created().ok(),
    }
}

/// `git log -1 --format=%cI` output as the card holds it: the trimmed date,
/// or `Untracked` for the empty answer git gives a file it never committed.
pub fn last_commit_from(stdout: &str) -> LastCommit {
    match stdout.trim() {
        "" => LastCommit::Untracked,
        date => LastCommit::At(date.to_string()),
    }
}

/// A byte count as the reader's border and the card write it: `812 B`,
/// `4.2 KiB`, `1.3 MiB`.
pub fn size_label(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    match bytes {
        b if b < KIB => format!("{b} B"),
        b if b < MIB => format!("{:.1} KiB", b as f64 / KIB as f64),
        b => format!("{:.1} MiB", b as f64 / MIB as f64),
    }
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

/// The heading of the section pinned first in the tree: every board.
pub const TODOS_SECTION: &str = "todos";

/// The project's personal todoz board, `<home>/notez/personal/<project>/
/// TODO.md`, when it is a file: one stat, the one file of the deferred
/// personal store that joins the tree (FZ-63). `<project>` is notez's name
/// for the repo: its root directory's name as a slug (`project_slug`); the
/// walked root is the git toplevel notez names it from.
pub fn personal_board(home: &Path, root: &Path) -> Option<PathBuf> {
    let project = project_slug(&root.file_name()?.to_string_lossy());
    if project.is_empty() {
        return None;
    }
    let board = home
        .join("notez")
        .join("personal")
        .join(project)
        .join("TODO.md");
    board.is_file().then_some(board)
}

/// notez's project name for a directory name, ported from notez-core's
/// `util::sanitize::name`: trimmed, lowercased, whitespace runs joined with
/// `-`, then only alphanumerics and `-` kept (`My_Repo` is `myrepo`).
fn project_slug(dir_name: &str) -> String {
    dir_name
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
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
    /// The walk of `root` with the personal board (`personal_board`) added,
    /// when there is one and the walk did not already find it: compared as
    /// resolved paths, so `~/notez` walked as a repo lists its own
    /// `personal/<project>/TODO.md` once.
    pub fn with_personal(mut self, root: &Path, board: Option<PathBuf>) -> Self {
        let resolved = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        if let Some(board) = board {
            let wanted = resolved(&board);
            if !self
                .files
                .iter()
                .any(|f| resolved(&root.join(&f.rel)) == wanted)
            {
                self.files.push(NoteFile {
                    rel: board,
                    notez: None,
                });
            }
        }
        self
    }

    /// The tree drawn for `all_markdown`, in drawing order. First, in both
    /// modes, the `todos` section: every board the walk found, by path,
    /// then the personal one, each named `TODO.md` (FZ-63); absent without
    /// a board. Then, notes-only: every notes directory a section, its
    /// files beneath and nested directories indented. All markdown: the
    /// same, and every other markdown file under a section for its
    /// directory. A board is never listed twice. Sections run shallowest
    /// first, then by path.
    pub fn rows(&self, all_markdown: bool) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut boards: Vec<usize> = (0..self.files.len())
            .filter(|&i| self.files[i].kind() == NoteKind::Todo)
            .collect();
        boards.sort_by_key(|&i| (self.files[i].is_personal(), &self.files[i].rel));
        if !boards.is_empty() {
            rows.push(Row::Section(TODOS_SECTION.to_string()));
        }
        for i in boards {
            let name = self.files[i]
                .rel
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            rows.push(Row::File {
                depth: 0,
                name,
                file: i,
            });
        }
        // (section dir, file index), the section being the notes directory
        // or, for loose markdown, the file's own directory.
        let mut placed: Vec<(PathBuf, usize)> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.kind() != NoteKind::Todo)
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

/// Read the first `BODY_CAP` bytes of `path` as text, for the reader. A
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

    /// FZ-63: the `todos` section comes first in both modes, the repo's
    /// boards by path and then the personal one from the fixture home; a
    /// board is not repeated under its directory. Without any board, and
    /// with a home that has none for this project, there is no section.
    #[test]
    fn the_todos_section_pins_every_board_once() {
        let parent = fixture(
            "boards",
            &[
                "My_Repo/TODO.md",
                "My_Repo/notez/TODO.md",
                "My_Repo/notez/plan.md",
                "My_Repo/docs/guide.md",
            ],
        );
        let root = parent.join("My_Repo");
        // notez names the project `myrepo`; the raw directory name is a
        // decoy the slug must not find.
        let home = fixture(
            "boards_home",
            &[
                "notez/personal/myrepo/TODO.md",
                "notez/personal/My_Repo/TODO.md",
                "notez/personal/other/TODO.md",
            ],
        );
        let personal = home.join("notez/personal/myrepo/TODO.md");
        assert_eq!(personal_board(&home, &root), Some(personal.clone()));
        let notes = walk(&root)
            .unwrap()
            .with_personal(&root, personal_board(&home, &root));

        assert_eq!(
            drawn(&notes.rows(false)),
            [
                "todos",
                "  TODO.md",
                "  TODO.md",
                "  TODO.md",
                "notez/",
                "  plan.md",
            ],
        );
        assert_eq!(
            drawn(&notes.rows(true)),
            [
                "todos",
                "  TODO.md",
                "  TODO.md",
                "  TODO.md",
                "docs/",
                "  guide.md",
                "notez/",
                "  plan.md",
            ],
        );
        let places: Vec<String> = notes.listed(true).iter().map(|f| f.place()).collect();
        assert_eq!(places, ["./", "notez/", "personal", "docs/", "notez/"]);
        assert_eq!(notes.listed(true)[2].rel, personal);

        // Added twice, listed once.
        let again = notes.clone().with_personal(&root, Some(personal));
        assert_eq!(again.files.len(), notes.files.len());

        let bare = fixture("boards_none", &["notez/plan.md"]);
        let notes = walk(&bare)
            .unwrap()
            .with_personal(&bare, personal_board(&home, &bare));
        assert_eq!(drawn(&notes.rows(false)), ["notez/", "  plan.md"]);
        for dir in [parent, home, bare] {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// notez's own name slug: `sanitize::name`'s rules.
    #[test]
    fn a_project_slug_is_notez_sanitized_name() {
        assert_eq!(project_slug("My_Repo"), "myrepo");
        assert_eq!(project_slug("  Big Project  Two "), "big-project-two");
        assert_eq!(project_slug("fleetz"), "fleetz");
        assert_eq!(project_slug("x.y-z"), "xy-z");
    }

    /// Review finding 2: the notez vault itself walked as a repo finds its
    /// own `personal/notez/TODO.md`, which is also its personal board; it
    /// is listed once.
    #[test]
    fn the_vault_walked_as_a_repo_lists_its_personal_board_once() {
        let home = fixture("vault_home", &["notez/personal/notez/TODO.md"]);
        let root = home.join("notez");
        let board = personal_board(&home, &root);
        assert_eq!(board, Some(home.join("notez/personal/notez/TODO.md")));
        let notes = walk(&root).unwrap().with_personal(&root, board);
        assert_eq!(drawn(&notes.rows(true)), ["todos", "  TODO.md"]);
        let _ = std::fs::remove_dir_all(&home);
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

    /// The ticket's four names, plus the case and path forms the rule must
    /// survive: `TODO.md` anywhere and in any case is a todo, a `.txt` is
    /// text, an image is other.
    #[test]
    fn a_note_kind_comes_from_its_name() {
        let kind = |rel: &str| {
            NoteFile {
                rel: rel.into(),
                notez: None,
            }
            .kind()
        };
        assert_eq!(kind("README.md"), NoteKind::Markdown);
        assert_eq!(kind("TODO.md"), NoteKind::Todo);
        assert_eq!(kind("notes.txt"), NoteKind::Text);
        assert_eq!(kind("diagram.png"), NoteKind::Other);
        assert_eq!(kind("notez/sub/todo.md"), NoteKind::Todo);
        assert_eq!(kind("notez/plan.MD"), NoteKind::Markdown);
        assert_eq!(kind("notez/Makefile"), NoteKind::Other);
        assert_eq!(NoteKind::Markdown.label(), "markdown");
    }

    /// The facts are the file's size and times, and git's empty answer is
    /// "untracked" (`None`), not an empty date.
    #[test]
    fn file_facts_read_the_metadata_and_an_empty_log_is_untracked() {
        let root = fixture("facts", &["notez/a.md"]);
        let facts = file_facts(&root.join("notez/a.md"));
        assert_eq!(facts.size, Some("# notez/a.md\n".len() as u64));
        assert!(facts.modified.is_some());
        assert_eq!(file_facts(&root.join("missing.md")), NoteFacts::default());

        assert_eq!(last_commit_from(""), LastCommit::Untracked);
        assert_eq!(last_commit_from("\n"), LastCommit::Untracked);
        assert_eq!(
            last_commit_from("2026-10-29T10:50:00+01:00\n"),
            LastCommit::At("2026-10-29T10:50:00+01:00".to_string())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_size_is_written_in_binary_units() {
        assert_eq!(size_label(0), "0 B");
        assert_eq!(size_label(812), "812 B");
        assert_eq!(size_label(4300), "4.2 KiB");
        assert_eq!(size_label(1024 * 1024 * 13 / 10), "1.3 MiB");
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
