use log::info;

use crate::{util::Pos, Link};
use std::collections::{VecDeque, HashMap};

pub type MaybeLinkIds = Option<Vec<usize>>;

#[derive(Clone, Debug)]
pub enum EditKind {
    InsertChar((char, MaybeLinkIds)),
    DeleteChar((char, MaybeLinkIds)),
    InsertLine((String, MaybeLinkIds)),
    DeleteLine((String, MaybeLinkIds)),
    InsertStr((String, MaybeLinkIds)),
    DeleteStr((String, MaybeLinkIds)),
    InsertChunk((Vec<String>, MaybeLinkIds)),
    DeleteChunk((Vec<String>, MaybeLinkIds)),
    InsertNewline,
    DeleteNewline,
}

impl EditKind {
    pub(crate) fn apply(
        &mut self,
        lines: &mut Vec<String>,
        links: &mut HashMap<usize, Link>,
        before: &Pos,
        after: &Pos,
    ) {
        match self {
            EditKind::InsertChar((c, link_vec)) => {
                lines[before.row].insert(before.offset, *c);
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        link.toggle_edited();
                        link.toggle_deleted();
                    } 
                }
            }
            EditKind::DeleteChar((_, link_vec)) => {
                lines[before.row].remove(after.offset);
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        link.toggle_deleted();
                    } 
                }
            }
            EditKind::InsertLine((line, link_vec)) => {
                lines.insert(before.row, line.to_string());
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        link.toggle_edited();
                        link.toggle_deleted();
                    } 
                }
            }
            EditKind::DeleteLine((_, link_vec)) => {
                lines.remove(before.row);
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        link.toggle_deleted();
                    } 
                }
            }
            EditKind::InsertStr((s, link_vec)) => {
                lines[before.row].insert_str(before.offset, s.as_str());
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        link.toggle_edited();
                        link.deleted = false;
                    } 
                }
            }
            EditKind::DeleteStr((s, link_vec)) => {
                lines[after.row].drain(after.offset..after.offset + s.len());
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        link.toggle_deleted();
                    } 
                }
            }
            EditKind::InsertChunk((c, link_vec)) => {
                info!("Inside InsertChunk");
                debug_assert!(c.len() > 1, "Chunk size must be > 1: {:?}", c);

                // Handle first line of chunk
                let first_line = &mut lines[before.row];
                let mut last_line = first_line.drain(before.offset..).as_str().to_string();
                first_line.push_str(&c[0]);

                // Handle last line of chunk
                let next_row = before.row + 1;
                last_line.insert_str(0, c.last().unwrap());
                lines.insert(next_row, last_line);

                // Handle middle lines of chunk
                lines.splice(next_row..next_row, c[1..c.len() - 1].iter().cloned());
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        info!("InsertChunk::link: {:?}", link);
                        link.toggle_edited();
                        link.deleted = false;
                        info!("InsertChunk::link AFTER .toggle_deleted/edited(): {:?}", link);
                    } 
                }
            }
            EditKind::DeleteChunk((c, link_vec)) => {
                info!("Inside DeleteChunk");
                debug_assert!(c.len() > 1, "Chunk size must be > 1: {:?}", c);
                // Remove middle lines of chunk
                let mut last_line = lines
                    .drain(after.row + 1..after.row + c.len())
                    .last()
                    .unwrap();
                // Remove last line of chunk
                last_line.drain(..c[c.len() - 1].len());

                // Remove first line of chunk and concat remaining
                let first_line = &mut lines[after.row];
                first_line.truncate(after.offset);
                first_line.push_str(&last_line);
                if let Some(l) = link_vec {
                    for id in l {
                        let link = links.get_mut(id).expect("link should be present");
                        info!("DeleteChunk::link: {:?}", link);
                        link.toggle_deleted();
                        info!("DeleteChunk::link AFTER .toggle_deleted(): {:?}", link);
                    } 
                }
            }
            EditKind::InsertNewline => {
                let line = &mut lines[before.row];
                let next_line = line[before.offset..].to_string();
                line.truncate(before.offset);
                lines.insert(before.row + 1, next_line);
            }
            EditKind::DeleteNewline => {
                debug_assert!(before.row > 0, "invalid pos: {:?}", before);
                let line = lines.remove(before.row);
                lines[before.row - 1].push_str(&line);
            }
        }
    }

    fn invert(&self) -> Self {
        use EditKind::*;
        match self.clone() {
            InsertChar((c, l)) => DeleteChar((c, l)),
            DeleteChar((c, l)) => InsertChar((c, l)),
            InsertLine((s, l)) => DeleteLine((s, l)),
            DeleteLine((s, l)) => InsertLine((s, l)),
            InsertStr((s, l)) => DeleteStr((s, l)),
            DeleteStr((s, l)) => InsertStr((s, l)),
            InsertChunk((c, l)) => DeleteChunk((c, l)),
            DeleteChunk((c, l)) => InsertChunk((c, l)),
            InsertNewline => DeleteNewline,
            DeleteNewline => InsertNewline,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Edit {
    kind: EditKind,
    before: Pos,
    after: Pos,
}

impl Edit {
    pub fn new(kind: EditKind, before: Pos, after: Pos) -> Self {
        Self {
            kind,
            before,
            after,
        }
    }

    pub fn redo(&mut self, lines: &mut Vec<String>, links: &mut HashMap<usize, Link>) {
        self.kind.apply(lines, links, &self.before, &self.after);
    }

    pub fn undo(&mut self, lines: &mut Vec<String>, links: &mut HashMap<usize, Link>) {
        self.kind.invert().apply(lines, links, &self.after, &self.before); // Undo is redo of inverted edit
    }

    pub fn cursor_before(&self) -> (usize, usize) {
        (self.before.row, self.before.col)
    }

    pub fn cursor_after(&self) -> (usize, usize) {
        (self.after.row, self.after.col)
    }
}

#[derive(Clone, Debug)]
pub struct History {
    index: usize,
    max_items: usize,
    edits: VecDeque<Edit>,
}

impl History {
    pub fn new(max_items: usize) -> Self {
        Self {
            index: 0,
            max_items,
            edits: VecDeque::new(),
        }
    }

    pub fn push(&mut self, edit: Edit) {
        if self.max_items == 0 {
            return;
        }

        if self.edits.len() == self.max_items {
            self.edits.pop_front();
            self.index = self.index.saturating_sub(1);
        }

        if self.index < self.edits.len() {
            self.edits.truncate(self.index);
        }

        self.index += 1;
        self.edits.push_back(edit);
    }

    pub fn redo(
        &mut self,
        lines: &mut Vec<String>,
        links: &mut HashMap<usize, Link>
    ) -> Option<((usize, usize), (usize, usize))> {
        if self.index == self.edits.len() {
            return None;
        }
        let edit = &mut self.edits[self.index];
        edit.redo(lines, links);
        self.index += 1;
        Some((edit.cursor_before(), edit.cursor_after()))
    }

    pub fn undo(
        &mut self,
        lines: &mut Vec<String>,
        links: &mut HashMap<usize, Link>
    ) -> Option<((usize, usize), (usize, usize))> {
        self.index = self.index.checked_sub(1)?;
        let edit = &mut self.edits[self.index];
        edit.undo(lines, links);
        Some((edit.cursor_before(), edit.cursor_after()))
    }

    pub fn max_items(&self) -> usize {
        self.max_items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_delete_chunk() {
        #[rustfmt::skip]
        let tests = [
            // Positions
            (
                // Text before edit
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                // (row, col) position before edit
                (0, 0),
                // Chunk to be inserted
                &[
                    "x", "y",
                ][..],
                // Text after edit
                &[
                    "x",
                    "yab",
                    "cd",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (0, 1),
                &[
                    "x", "y",
                ][..],
                &[
                    "ax",
                    "yb",
                    "cd",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (0, 2),
                &[
                    "x", "y",
                ][..],
                &[
                    "abx",
                    "y",
                    "cd",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (1, 0),
                &[
                    "x", "y",
                ][..],
                &[
                    "ab",
                    "x",
                    "ycd",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (1, 1),
                &[
                    "x", "y",
                ][..],
                &[
                    "ab",
                    "cx",
                    "yd",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (1, 2),
                &[
                    "x", "y",
                ][..],
                &[
                    "ab",
                    "cdx",
                    "y",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (2, 0),
                &[
                    "x", "y",
                ][..],
                &[
                    "ab",
                    "cd",
                    "x",
                    "yef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (2, 1),
                &[
                    "x", "y",
                ][..],
                &[
                    "ab",
                    "cd",
                    "ex",
                    "yf",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (2, 2),
                &[
                    "x", "y",
                ][..],
                &[
                    "ab",
                    "cd",
                    "efx",
                    "y",
                ][..],
            ),
            // More than 2 lines
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (1, 1),
                &[
                    "x", "y", "z", "w"
                ][..],
                &[
                    "ab",
                    "cx",
                    "y",
                    "z",
                    "wd",
                    "ef",
                ][..],
            ),
            // Empty lines
            (
                &[
                    "",
                    "",
                    "",
                ][..],
                (0, 0),
                &[
                    "x", "y", "z"
                ][..],
                &[
                    "x",
                    "y",
                    "z",
                    "",
                    "",
                ][..],
            ),
            (
                &[
                    "",
                    "",
                    "",
                ][..],
                (1, 0),
                &[
                    "x", "y", "z"
                ][..],
                &[
                    "",
                    "x",
                    "y",
                    "z",
                    "",
                ][..],
            ),
            (
                &[
                    "",
                    "",
                    "",
                ][..],
                (2, 0),
                &[
                    "x", "y", "z"
                ][..],
                &[
                    "",
                    "",
                    "x",
                    "y",
                    "z",
                ][..],
            ),
            // Empty buffer
            (
                &[
                    "",
                ][..],
                (0, 0),
                &[
                    "x", "y", "z"
                ][..],
                &[
                    "x",
                    "y",
                    "z",
                ][..],
            ),
            // Insert empty lines
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (0, 0),
                &[
                    "", "", "",
                ][..],
                &[
                    "",
                    "",
                    "ab",
                    "cd",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (1, 0),
                &[
                    "", "", "",
                ][..],
                &[
                    "ab",
                    "",
                    "",
                    "cd",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (1, 1),
                &[
                    "", "", "",
                ][..],
                &[
                    "ab",
                    "c",
                    "",
                    "d",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (1, 2),
                &[
                    "", "", "",
                ][..],
                &[
                    "ab",
                    "cd",
                    "",
                    "",
                    "ef",
                ][..],
            ),
            (
                &[
                    "ab",
                    "cd",
                    "ef",
                ][..],
                (2, 2),
                &[
                    "", "", "",
                ][..],
                &[
                    "ab",
                    "cd",
                    "ef",
                    "",
                    "",
                ][..],
            ),
            // Multi-byte characters
            (
                &[
                    "🐶🐱",
                    "🐮🐰",
                    "🐧🐭",
                ][..],
                (0, 0),
                &[
                    "🐷", "🐼", "🐴",
                ][..],
                &[
                    "🐷",
                    "🐼",
                    "🐴🐶🐱",
                    "🐮🐰",
                    "🐧🐭",
                ][..],
            ),
            (
                &[
                    "🐶🐱",
                    "🐮🐰",
                    "🐧🐭",
                ][..],
                (0, 2),
                &[
                    "🐷", "🐼", "🐴",
                ][..],
                &[
                    "🐶🐱🐷",
                    "🐼",
                    "🐴",
                    "🐮🐰",
                    "🐧🐭",
                ][..],
            ),
            (
                &[
                    "🐶🐱",
                    "🐮🐰",
                    "🐧🐭",
                ][..],
                (1, 0),
                &[
                    "🐷", "🐼", "🐴",
                ][..],
                &[
                    "🐶🐱",
                    "🐷",
                    "🐼",
                    "🐴🐮🐰",
                    "🐧🐭",
                ][..],
            ),
            (
                &[
                    "🐶🐱",
                    "🐮🐰",
                    "🐧🐭",
                ][..],
                (1, 1),
                &[
                    "🐷", "🐼", "🐴",
                ][..],
                &[
                    "🐶🐱",
                    "🐮🐷",
                    "🐼",
                    "🐴🐰",
                    "🐧🐭",
                ][..],
            ),
            (
                &[
                    "🐶🐱",
                    "🐮🐰",
                    "🐧🐭",
                ][..],
                (2, 2),
                &[
                    "🐷", "🐼", "🐴",
                ][..],
                &[
                    "🐶🐱",
                    "🐮🐰",
                    "🐧🐭🐷",
                    "🐼",
                    "🐴",
                ][..],
            ),
        ];

        for test in tests {
            let (before, pos, input, expected) = test;
            let (row, col) = pos;
            let before_pos = {
                let offset = before[row]
                    .char_indices()
                    .map(|(i, _)| i)
                    .nth(col)
                    .unwrap_or(before[row].len());
                Pos::new(row, col, offset)
            };
            let mut lines: Vec<_> = before.iter().map(|s| s.to_string()).collect();
            let mut links: HashMap<usize, Link> = HashMap::new();
            let chunk: Vec<_> = input.iter().map(|s| s.to_string()).collect();
            let after_pos = {
                let row = row + input.len() - 1;
                let last = input.last().unwrap();
                let col = last.chars().count();
                Pos::new(row, col, last.len())
            };

            let mut edit = EditKind::InsertChunk((chunk.clone(), None));
            edit.apply(&mut lines, &mut links, &before_pos, &after_pos);
            assert_eq!(&lines, expected, "{test:?}");

            let mut edit = EditKind::DeleteChunk((chunk, None));
            edit.apply(&mut lines, &mut links, &after_pos, &before_pos);
            assert_eq!(&lines, &before, "{test:?}");
        }
    }
}
