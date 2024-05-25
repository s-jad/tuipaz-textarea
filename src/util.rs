pub fn spaces(size: u8) -> &'static str {
    const SPACES: &str = "                                                                                                                                                                                                                                                                ";
    &SPACES[..size as usize]
}

pub fn num_digits(i: usize) -> u8 {
    f64::log10(i as f64) as u8 + 1
}

#[derive(Debug, Clone)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
    pub offset: usize,
}

impl Pos {
    pub fn new(row: usize, col: usize, offset: usize) -> Self {
        Self { row, col, offset }
    }
}

pub(crate) fn log_format<T: std::fmt::Debug>(data: &T, prefix: &str) -> String {
    let mut s = String::new();
    s.push_str(prefix);
    s.push_str(": ");
    s.push_str(&format!("{:?}", data));
    s
}
