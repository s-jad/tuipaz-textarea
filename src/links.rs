#[derive(Debug, Clone, Copy)]
pub struct Link {
    pub id: usize,
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

impl Link {
    pub(crate) fn new(id: usize, row: usize, start_col: usize, end_col: usize) -> Self {
        Self {
            id,
            row,
            start_col,
            end_col,
        }
    }
}
