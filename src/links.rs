#[derive(Debug, Clone, Eq, Copy)]
pub struct LinkPos {
    pub row: usize,
    pub col: usize,
}

impl PartialEq for LinkPos {
    fn eq(&self, other: &Self) -> bool {
        self.row == other.row && self.col == other.col
    }
}

impl PartialOrd for LinkPos {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.row.partial_cmp(&other.row) {
            Some(core::cmp::Ordering::Equal) => self.col.partial_cmp(&other.col),
            ord => return ord,
        }
    }
}

impl Ord for LinkPos {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl LinkPos {
    pub(crate) fn new((row, col): (usize, usize)) -> Self {
        Self { row, col }
    }

    pub(crate) fn order_positions(self, other: LinkPos) -> (LinkPos, LinkPos) {
        (std::cmp::min(self, other), std::cmp::max(self, other))
    }
}

#[derive(Debug, Clone)]
pub struct Link {
    pub id: usize,
    pub start: LinkPos,
    pub end: LinkPos,
}

impl Link {
    pub(crate) fn new(id: usize, start: LinkPos, end: LinkPos) -> Self {
        Self { id, start, end }
    }
}
