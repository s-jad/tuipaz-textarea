use std::{cell::{RefCell, Cell}, rc::Rc};

use crate::ratatui::style::{Color, Style};
use log::info;
use regex::Regex;

#[derive(Clone, Debug)]
pub struct MatchIndex {
    pub(crate) idx: usize,
    pub(crate) pos: (usize, usize)
}

impl MatchIndex {
    fn new(idx: usize,  pos: (usize, usize)) -> Self {
        Self {
            idx,
            pos,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Hop {
    pub pat: Option<Regex>,
    pub style: Style,
    pub count: Cell<usize>,
    pub match_indexes: Rc<RefCell<Vec<MatchIndex>>>,
}

impl Default for Hop {
    fn default() -> Self {
        Self {
            pat: None,
            style: Style::default().bg(Color::Red),
            // count set to 10 to ensure all idx are double-digit
            count: Cell::new(10),
            match_indexes: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl Hop {
    pub fn matches<'a>(
        &'a self,
        line: &'a str,
    ) -> Option<impl Iterator<Item = (usize, usize)> + 'a> {
        let pat = self.pat.as_ref()?;
        let matches = pat.find_iter(line).map(|m| (m.start(), m.end()));
        Some(matches)
    }

    pub fn set_pattern(&mut self, query: &str) -> Result<(), regex::Error> {
        match &self.pat {
            Some(r) if r.as_str() == query => {}
            _ if query.is_empty() => self.pat = None,
            _ => self.pat = Some(Regex::new(query)?),
        }
        Ok(())
    }

    pub fn clear_pattern(&mut self) {
        self.pat = None;
    }

    pub fn reset_count(&self) {
        self.count.set(10);
    }

    pub fn set_count(&self, count: usize) {
        self.count.set(count);
    }

    pub fn get_count(&self) -> usize {
        self.count.get()
    }

    pub fn set_match_indexes(&self, matches: Vec<(usize, usize)>, row: usize) {
        for (start, count) in matches {
            let match_idx = MatchIndex::new(count, (row, start));
            self.match_indexes.borrow_mut().push(match_idx);
        }
    }

    pub fn clear_match_indexes(&self) {
        self.match_indexes.borrow_mut().drain(..);
    }
}
