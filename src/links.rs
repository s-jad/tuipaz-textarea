#[derive(Debug, Clone, Copy)]
pub struct Link {
    pub id: usize,
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub edited: bool,
    pub deleted: bool,
}

impl Link {
    pub(crate) fn new(id: usize, row: usize, start_col: usize, end_col: usize) -> Self {
        Self {
            id,
            row,
            start_col,
            end_col,
            edited: false,
            deleted: false,
        }
    }


    pub(crate) fn toggle_edited(&mut self) {
        self.edited = !self.edited;
    }

    pub(crate) fn toggle_deleted(&mut self) {
        self.deleted = !self.deleted;
    }
}

impl PartialEq for Link {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
