//! Commit-graph layout: which lane each commit occupies, and which
//! box-drawing character lands in each cell of the graph column.
//!
//! Computed from parent hashes, not read out of `git log --graph`. That is the
//! whole point of the module. Git's graph is a picture, and a picture has two
//! properties this pane cannot live with:
//!
//! - It emits filler rows. A fan-out or a collapse gets a line of its own with
//!   no commit on it, so the hash, author and subject columns have to be held
//!   open on a row that says nothing. Here a merge fans out *on* the merge
//!   commit's own row, and the output is one row per commit, always.
//! - It packs a diagonal into a single column, which cannot be translated to
//!   box drawing: a `╯` with no `─` to its left is punctuation floating next
//!   to the line it is supposed to join. Cells here are two columns wide and
//!   the second column carries the connector.
//!
//! Ported from lazygit (<https://github.com/jesseduffield/lazygit>), MIT,
//! Copyright (c) 2018 Jesse Duffield: `pkg/gui/presentation/graph/graph.go`
//! and `cell.go` at tag `v0.59.0`, with nine of `graph_test.go`'s fixtures
//! ported alongside them below. The pipe model, the lane-allocation rules and
//! the box-drawing table are lazygit's design; deviations are marked where
//! they occur. Full notice in `NOTICES.md` at the repo root.

/// Git's empty-tree hash, standing in as the parent of a root commit so its
/// pipe has somewhere to point. No commit can carry it, so the pipe simply
/// never terminates, which is the correct picture: a root has nothing above it.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Sentinel ancestor of the newest commit, so the first row has a pipe to
/// terminate and therefore lands in lane 0 rather than being treated as an
/// unreferenced root.
const START: &str = "START";

/// An ordinary commit's node.
///
/// Git draws every commit as `*`, a heavy glyph repeated on every row that
/// makes a long history read as noise. A small circle carries the same meaning
/// at a fraction of the weight, and leaves the emphatic shape free to mean the
/// one thing worth picking out: a merge.
///
/// U+26AC is East Asian Narrow, and that is not incidental. The graph column
/// is tiled arithmetic padded to the widest row, so an Ambiguous-width glyph
/// would be one column in some terminals and two in others, and every row
/// after it would misalign. The obvious candidates (`●` `○` `•` `◆` `⊙`) are
/// all Ambiguous, as are lazygit's own `◯` and `⏣`; this one and `◦` `∘` `◊`
/// are not. The *lanes* are box drawing and Ambiguous, which is safe for a
/// different reason: every pane border in the app is already drawn with the
/// same characters, so a terminal that got their width wrong would have broken
/// fleetz long before this.
pub const COMMIT_NODE: char = '⚬';

/// A merge's node: the asterisk git already used, circled, so the column reads
/// as one family of nodes while merges still stand out. Narrow, like
/// `COMMIT_NODE`.
pub const MERGE_NODE: char = '⊛';

/// One commit as the layout needs to see it: an identity and its parents.
/// Two or more parents is a merge.
#[derive(Debug, Clone, Copy)]
pub struct Commit<'a> {
    pub hash: &'a str,
    pub parents: &'a [String],
}

/// One lane of one row, two terminal columns wide.
///
/// The second column is where the horizontal `─` goes. Without it a fan-out is
/// a row of detached corners; with it the same row reads as a drawn line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The node or vertical character.
    pub glyph: char,
    /// The horizontal run carrying the line into the next lane, or a space.
    pub connector: char,
    /// Palette slot for `glyph`. See `ui::colors::graph_lane_color`.
    pub style: u8,
    /// Palette slot for `connector`, which is often a different branch's.
    pub connector_style: u8,
}

/// What a pipe does on the row being drawn. The discriminant order is
/// load-bearing: pipes are sorted by it, and terminating pipes must sort ahead
/// of starting ones at the same position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PipeKind {
    /// Ends at this row's commit.
    Terminates,
    /// Begins at this row's commit.
    Starts,
    /// Passes through, possibly sliding sideways into a freed lane.
    Continues,
}

/// A single strand of history in flight: where it enters the row, where it
/// leaves, and which commits it connects.
#[derive(Debug, Clone)]
struct Pipe {
    from_pos: i16,
    to_pos: i16,
    from_hash: String,
    to_hash: String,
    kind: PipeKind,
    style: u8,
}

impl Pipe {
    fn left(&self) -> i16 {
        self.from_pos.min(self.to_pos)
    }

    fn right(&self) -> i16 {
        self.from_pos.max(self.to_pos)
    }
}

/// Lay out `commits` (newest first, as `git log` yields them) into one row of
/// cells per commit.
///
/// Rows vary in width: a row is only as wide as the lanes in flight when it
/// was drawn. Padding them into a column is the renderer's job, since only it
/// knows what the rest of the row costs.
pub fn layout(commits: &[Commit<'_>]) -> Vec<Vec<Cell>> {
    if commits.is_empty() {
        return Vec::new();
    }
    let mut pipes = vec![Pipe {
        from_pos: 0,
        to_pos: 0,
        from_hash: START.to_string(),
        to_hash: commits[0].hash.to_string(),
        kind: PipeKind::Starts,
        style: 0,
    }];
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        pipes = next_pipes(&pipes, commit);
        rows.push(render_row(&pipes));
    }
    rows
}

/// Which palette slot a branch born in lane `pos` gets.
fn lane_style(pos: i16) -> u8 {
    pos.clamp(0, u8::MAX as i16) as u8
}

/// The lowest lane index not in `occupied`.
fn lowest_free(occupied: &[i16]) -> i16 {
    let mut i = 0;
    while occupied.contains(&i) {
        i += 1;
    }
    i
}

/// Record that a pipe crossed every lane between `from` and `to` and came to
/// rest on `to`, so nothing else is routed over the top of it.
fn traverse(traversed: &mut Vec<i16>, taken: &mut Vec<i16>, from: i16, to: i16) {
    for i in from.min(to)..=from.max(to) {
        if !traversed.contains(&i) {
            traversed.push(i);
        }
    }
    if !taken.contains(&to) {
        taken.push(to);
    }
}

/// Advance the pipe set by one commit.
fn next_pipes(prev: &[Pipe], commit: &Commit<'_>) -> Vec<Pipe> {
    let max_pos = prev.iter().map(|p| p.to_pos).max().unwrap_or(0);

    // A pipe that terminated on the previous row has no bearing on this one.
    let current: Vec<&Pipe> = prev
        .iter()
        .filter(|p| p.kind != PipeKind::Terminates)
        .collect();

    // This commit sits under the first pipe pointing at it. Falling through to
    // `max_pos + 1` means nothing references it, which happens only when the
    // log spans several branches and this is the tip of one of them.
    let mut pos = max_pos + 1;
    let mut inherited = None;
    for p in &current {
        if p.to_hash == commit.hash {
            pos = p.to_pos;
            // Deviation from lazygit: it colours pipes by author, which paints
            // a single-maintainer repo in one flat hue. Inheriting instead
            // means a branch keeps the colour it was born with even as it
            // migrates lanes, which is what colouring by lane alone gets wrong.
            inherited = Some(p.style);
            break;
        }
    }
    let style = inherited.unwrap_or_else(|| lane_style(pos));

    // A taken spot is one a pipe is ending on; a traversed spot is one a pipe
    // starts on, ends on, or crosses.
    let mut taken: Vec<i16> = Vec::new();
    let mut traversed: Vec<i16> = Vec::new();

    let first_parent = commit
        .parents
        .first()
        .map(String::as_str)
        .unwrap_or(EMPTY_TREE);

    let mut next = Vec::with_capacity(current.len() + commit.parents.len());
    next.push(Pipe {
        from_pos: pos,
        to_pos: pos,
        from_hash: commit.hash.to_string(),
        to_hash: first_parent.to_string(),
        kind: PipeKind::Starts,
        style,
    });

    // Lanes a continuing pipe already owns. A newly opened merge pipe may not
    // land on one, or the two would be drawn on top of each other.
    let continuing: Vec<i16> = current
        .iter()
        .filter(|p| p.to_hash != commit.hash)
        .map(|p| p.to_pos)
        .collect();

    for p in &current {
        if p.to_hash == commit.hash {
            next.push(Pipe {
                from_pos: p.to_pos,
                to_pos: pos,
                from_hash: p.from_hash.clone(),
                to_hash: p.to_hash.clone(),
                kind: PipeKind::Terminates,
                style: p.style,
            });
            traverse(&mut traversed, &mut taken, p.to_pos, pos);
        } else if p.to_pos < pos {
            let avail = lowest_free(&traversed);
            next.push(Pipe {
                from_pos: p.to_pos,
                to_pos: avail,
                from_hash: p.from_hash.clone(),
                to_hash: p.to_hash.clone(),
                kind: PipeKind::Continues,
                style: p.style,
            });
            traverse(&mut traversed, &mut taken, p.to_pos, avail);
        }
    }

    // A merge fans out one new pipe per extra parent, on this same row rather
    // than on a filler row of its own.
    for parent in commit.parents.iter().skip(1) {
        let mut blocked = taken.clone();
        blocked.extend_from_slice(&continuing);
        let avail = lowest_free(&blocked);
        next.push(Pipe {
            from_pos: pos,
            to_pos: avail,
            from_hash: commit.hash.to_string(),
            to_hash: parent.clone(),
            // A merged-in branch takes the colour of the lane it fans out
            // into, and keeps it until it terminates.
            kind: PipeKind::Starts,
            style: lane_style(avail),
        });
        if !taken.contains(&avail) {
            taken.push(avail);
        }
    }

    // Anything to the right carries on, sliding left into whatever gap it can
    // reach without crossing something already drawn.
    for p in &current {
        if p.to_hash != commit.hash && p.to_pos > pos {
            let mut last = p.to_pos;
            let mut i = p.to_pos;
            while i > pos {
                if taken.contains(&i) || traversed.contains(&i) {
                    break;
                }
                last = i;
                i -= 1;
            }
            next.push(Pipe {
                from_pos: p.to_pos,
                to_pos: last,
                from_hash: p.from_hash.clone(),
                to_hash: p.to_hash.clone(),
                kind: PipeKind::Continues,
                style: p.style,
            });
            traverse(&mut traversed, &mut taken, p.to_pos, last);
        }
    }

    // Stable, unlike upstream's sort: pipes tying on both position and kind
    // then keep the order they were created in, so the colour a shared cell
    // ends up with is deterministic rather than left to the sort.
    next.sort_by(|a, b| a.to_pos.cmp(&b.to_pos).then(a.kind.cmp(&b.kind)));
    next
}

/// One lane carrying a plain commit node: what `layout` produces for every row
/// of a linear history. For fixtures elsewhere that need a graph row but are
/// not testing topology.
#[cfg(test)]
pub fn commit_cell() -> Cell {
    Cell {
        glyph: COMMIT_NODE,
        connector: ' ',
        style: 0,
        connector_style: 0,
    }
}

/// Which node, if any, a cell carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Commit,
    Merge,
}

/// A cell mid-construction: which of its four sides a line reaches.
#[derive(Debug, Clone, Copy, Default)]
struct CellBuilder {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    node: Option<NodeKind>,
    style: u8,
    right_style: Option<u8>,
}

impl CellBuilder {
    fn set_up(&mut self, style: u8) {
        self.up = true;
        self.style = style;
    }

    fn set_down(&mut self, style: u8) {
        self.down = true;
        self.style = style;
    }

    fn set_left(&mut self, style: u8) {
        self.left = true;
        // A vertical line trumps a horizontal one: where a branch crosses a
        // lane that is carrying on downwards, the lane keeps its own colour.
        if !self.up && !self.down {
            self.style = style;
        }
    }

    fn set_right(&mut self, style: u8, override_existing: bool) {
        self.right = true;
        if self.right_style.is_none() || override_existing {
            self.right_style = Some(style);
        }
    }

    fn finish(self) -> Cell {
        let (line, connector) = box_chars(self.up, self.down, self.left, self.right);
        let glyph = match self.node {
            Some(NodeKind::Commit) => COMMIT_NODE,
            Some(NodeKind::Merge) => MERGE_NODE,
            None => line,
        };
        Cell {
            glyph,
            connector,
            style: self.style,
            connector_style: self.right_style.unwrap_or(self.style),
        }
    }
}

/// The character a cell gets from the sides a line reaches, plus the connector
/// that carries it into the next lane.
fn box_chars(up: bool, down: bool, left: bool, right: bool) -> (char, char) {
    match (up, down, left, right) {
        (true, true, _, true) => ('│', '─'),
        (true, true, _, false) => ('│', ' '),
        (true, false, true, true) => ('┴', '─'),
        (true, false, true, false) => ('╯', ' '),
        (true, false, false, true) => ('╰', '─'),
        (true, false, false, false) => ('╵', ' '),
        (false, true, true, true) => ('┬', '─'),
        (false, true, true, false) => ('╮', ' '),
        (false, true, false, true) => ('╭', '─'),
        (false, true, false, false) => ('╷', ' '),
        (false, false, true, true) => ('─', '─'),
        (false, false, true, false) => ('─', ' '),
        (false, false, false, true) => ('╶', '─'),
        (false, false, false, false) => (' ', ' '),
    }
}

/// Rasterise one pipe set into the row's cells.
fn render_row(pipes: &[Pipe]) -> Vec<Cell> {
    let mut max_pos = 0i16;
    let mut commit_pos = 0i16;
    let mut start_count = 0usize;
    for p in pipes {
        match p.kind {
            PipeKind::Starts => {
                start_count += 1;
                commit_pos = p.from_pos;
            }
            PipeKind::Terminates => commit_pos = p.to_pos,
            PipeKind::Continues => {}
        }
        max_pos = max_pos.max(p.right());
    }
    // Every pipe a commit starts leaves from its own lane, so more than one
    // means it fanned out: a merge.
    let is_merge = start_count > 1;

    let mut cells = vec![CellBuilder::default(); (max_pos + 1) as usize];

    // Starting pipes first, and they override the connector colour: a fan-out
    // should read in the colour of the branch it creates, not the one it
    // happens to cross on the way there.
    for p in pipes.iter().filter(|p| p.kind == PipeKind::Starts) {
        draw(&mut cells, p, true);
    }
    for p in pipes.iter().filter(|p| p.kind != PipeKind::Starts) {
        // A pipe that both leaves and arrives on the commit's own lane draws
        // nothing: the node is already there, and drawing over it would erase
        // the one glyph the row exists to show.
        if p.kind == PipeKind::Terminates && p.from_pos == commit_pos && p.to_pos == commit_pos {
            continue;
        }
        draw(&mut cells, p, false);
    }

    cells[commit_pos as usize].node = Some(match is_merge {
        true => NodeKind::Merge,
        false => NodeKind::Commit,
    });

    cells.into_iter().map(CellBuilder::finish).collect()
}

/// Stamp one pipe onto the row.
fn draw(cells: &mut [CellBuilder], pipe: &Pipe, override_right: bool) {
    let (left, right) = (pipe.left(), pipe.right());
    if left != right {
        for cell in cells
            .iter_mut()
            .take(right as usize)
            .skip(left as usize + 1)
        {
            cell.set_left(pipe.style);
            cell.set_right(pipe.style, override_right);
        }
        cells[left as usize].set_right(pipe.style, override_right);
        cells[right as usize].set_left(pipe.style);
    }
    if matches!(pipe.kind, PipeKind::Starts | PipeKind::Continues) {
        cells[pipe.to_pos as usize].set_down(pipe.style);
    }
    if matches!(pipe.kind, PipeKind::Terminates | PipeKind::Continues) {
        cells[pipe.from_pos as usize].set_up(pipe.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay out a `(hash, parents)` spec and render each row the way lazygit's
    /// own tests do: the hash, a space, then two characters per cell, trimmed.
    fn rows(spec: &[(&str, &[&str])]) -> Vec<String> {
        let parents: Vec<Vec<String>> = spec
            .iter()
            .map(|(_, ps)| ps.iter().map(|s| (*s).to_string()).collect())
            .collect();
        let commits: Vec<Commit<'_>> = spec
            .iter()
            .zip(&parents)
            .map(|((hash, _), parents)| Commit { hash, parents })
            .collect();
        layout(&commits)
            .iter()
            .zip(spec)
            .map(|(cells, (hash, _))| {
                let mut line = format!("{hash} ");
                for cell in cells {
                    line.push(cell.glyph);
                    line.push(cell.connector);
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    fn assert_graph(spec: &[(&str, &[&str])], expected: &str) {
        let want: Vec<String> = expected.trim().lines().map(|l| l.trim().into()).collect();
        let got = rows(spec);
        assert_eq!(
            got,
            want,
            "\n got:\n{}\nwant:\n{}\n",
            got.join("\n"),
            want.join("\n")
        );
    }

    // The fixtures below are lazygit's own, from
    // pkg/gui/presentation/graph/graph_test.go at v0.59.0 (MIT, Copyright (c)
    // 2018 Jesse Duffield), with its node glyphs swapped for ours. They are the reason to port the algorithm
    // rather than approximate it: the topology cases they cover (a lane with
    // room to move left, a merge filling a gap before a continuing pipe, an
    // unreferenced tip) are exactly the ones that are easy to get subtly
    // wrong and hard to notice. The upstream cases exercising lazygit's
    // selected-commit highlight are not ported, because fleetz marks the
    // selected row with a wash and a caret instead and the graph column takes
    // no part in it.

    #[test]
    fn lays_out_a_history_with_some_merges() {
        assert_graph(
            &[
                ("1", &["2"]),
                ("2", &["3"]),
                ("3", &["4"]),
                ("4", &["5", "7"]),
                ("7", &["5"]),
                ("5", &["8"]),
                ("8", &["9"]),
                ("9", &["A", "B"]),
                ("B", &["D"]),
                ("D", &["D"]),
                ("A", &["E"]),
                ("E", &["F"]),
                ("F", &["D"]),
                ("D", &["G"]),
            ],
            "
            1 ⚬
            2 ⚬
            3 ⚬
            4 ⊛─╮
            7 │ ⚬
            5 ⚬─╯
            8 ⚬
            9 ⊛─╮
            B │ ⚬
            D │ ⚬
            A ⚬ │
            E ⚬ │
            F ⚬ │
            D ⚬─╯",
        );
    }

    #[test]
    fn a_lane_with_room_moves_left() {
        assert_graph(
            &[
                ("1", &["2"]),
                ("2", &["3", "4"]),
                ("4", &["3", "5"]),
                ("3", &["5"]),
                ("5", &["6"]),
                ("6", &["7"]),
            ],
            "
            1 ⚬
            2 ⊛─╮
            4 │ ⊛─╮
            3 ⚬─╯ │
            5 ⚬───╯
            6 ⚬",
        );
    }

    /// A commit nothing in the window points at -- the tip of another branch
    /// -- is tacked on past the rightmost lane rather than displacing one.
    #[test]
    fn an_unreferenced_tip_opens_a_lane_of_its_own() {
        assert_graph(
            &[
                ("1", &["2"]),
                ("2", &["3", "4"]),
                ("4", &["3", "5"]),
                ("Z", &["Z"]),
                ("3", &["5"]),
                ("5", &["6"]),
                ("6", &["7"]),
            ],
            "
            1 ⚬
            2 ⊛─╮
            4 │ ⊛─╮
            Z │ │ │ ⚬
            3 ⚬─╯ │ │
            5 ⚬───╯ │
            6 ⚬ ╭───╯",
        );
    }

    #[test]
    fn a_lane_with_room_moves_left_and_carries_on() {
        assert_graph(
            &[
                ("1", &["2"]),
                ("2", &["3", "4"]),
                ("3", &["5", "4"]),
                ("5", &["7", "8"]),
                ("4", &["7"]),
                ("7", &["11"]),
            ],
            "
            1 ⚬
            2 ⊛─╮
            3 ⊛─│─╮
            5 ⊛─│─│─╮
            4 │ ⚬─╯ │
            7 ⚬─╯ ╭─╯",
        );
    }

    #[test]
    fn several_lanes_collapse_onto_one_commit() {
        assert_graph(
            &[
                ("1", &["2"]),
                ("2", &["3", "4"]),
                ("3", &["5", "4"]),
                ("5", &["7", "8"]),
                ("7", &["4", "A"]),
                ("4", &["B"]),
                ("B", &["C"]),
            ],
            "
            1 ⚬
            2 ⊛─╮
            3 ⊛─│─╮
            5 ⊛─│─│─╮
            7 ⊛─│─│─│─╮
            4 ⚬─┴─╯ │ │
            B ⚬ ╭───╯ │",
        );
    }

    #[test]
    fn a_merge_whose_parent_is_the_very_next_commit() {
        assert_graph(
            &[
                ("1", &["2", "3"]),
                ("3", &["2"]),
                ("2", &["4", "5"]),
                ("4", &["6", "7"]),
                ("6", &["8"]),
            ],
            "
            1 ⊛─╮
            3 │ ⚬
            2 ⊛─│
            4 ⊛─│─╮
            6 ⚬ │ │",
        );
    }

    /// An octopus merge, and the case that motivates
    /// `lowest_free` being asked to skip lanes a continuing pipe already owns.
    #[test]
    fn a_new_merge_lane_fills_the_gap_before_a_continuing_one() {
        assert_graph(
            &[
                ("1", &["2", "3", "4", "5"]),
                ("4", &["2"]),
                ("2", &["A"]),
                ("A", &["6", "B"]),
                ("B", &["C"]),
            ],
            "
            1 ⊛─┬─┬─╮
            4 │ │ ⚬ │
            2 ⚬─│─╯ │
            A ⊛─│─╮ │
            B │ │ ⚬ │",
        );
    }

    #[test]
    fn freed_lanes_are_reclaimed_one_row_at_a_time() {
        assert_graph(
            &[
                ("1", &["2"]),
                ("2", &["3", "4"]),
                ("3", &["5", "4"]),
                ("5", &["7", "8"]),
                ("7", &["4", "A"]),
                ("4", &["B"]),
                ("B", &["C"]),
                ("C", &["D"]),
            ],
            "
            1 ⚬
            2 ⊛─╮
            3 ⊛─│─╮
            5 ⊛─│─│─╮
            7 ⊛─│─│─│─╮
            4 ⚬─┴─╯ │ │
            B ⚬ ╭───╯ │
            C ⚬ │ ╭───╯",
        );
    }

    #[test]
    fn six_lanes_unwind_in_order() {
        assert_graph(
            &[
                ("1", &["2"]),
                ("2", &["3", "4"]),
                ("3", &["5", "4"]),
                ("5", &["7", "G"]),
                ("7", &["8", "A"]),
                ("8", &["4", "E"]),
                ("4", &["B"]),
                ("B", &["C"]),
                ("C", &["D"]),
                ("D", &["F"]),
            ],
            "
            1 ⚬
            2 ⊛─╮
            3 ⊛─│─╮
            5 ⊛─│─│─╮
            7 ⊛─│─│─│─╮
            8 ⊛─│─│─│─│─╮
            4 ⚬─┴─╯ │ │ │
            B ⚬ ╭───╯ │ │
            C ⚬ │ ╭───╯ │
            D ⚬ │ │ ╭───╯",
        );
    }

    /// The property the whole module exists for. `git log --graph` emits a
    /// filler row for every fan-out and every collapse, which is what forced
    /// the pane to hold the hash, author and subject columns open on rows
    /// carrying no commit.
    #[test]
    fn every_row_is_a_commit_and_every_commit_is_a_row() {
        let spec: &[(&str, &[&str])] = &[
            ("1", &["2", "3"]),
            ("3", &["4", "5"]),
            ("4", &["2"]),
            ("2", &["6"]),
            ("5", &["6"]),
            ("6", &[]),
        ];
        assert_eq!(rows(spec).len(), spec.len());
    }

    /// A root commit has no parent to point at, so its pipe aims at the empty
    /// tree and simply never terminates. It must still draw as a node.
    #[test]
    fn a_root_commit_still_gets_a_node() {
        assert_graph(
            &[("1", &["2"]), ("2", &[])],
            "
            1 ⚬
            2 ⚬",
        );
    }
}
