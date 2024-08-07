use log::info;

use crate::cursor::CursorMove;
use crate::highlight::LineHighlighter;
use crate::history::{Edit, EditKind, History};
use crate::hop::Hop;
use crate::input::{Input, Key};
use crate::links::Link;
use crate::ratatui::layout::Alignment;
use crate::ratatui::style::{Color, Modifier, Style};
use crate::ratatui::widgets::{Block, Widget};
use crate::scroll::Scrolling;
use crate::search::Search;
use crate::util::{log_format, spaces, Pos};
use crate::widget::{Renderer, Viewport};
use crate::word::{find_word_end_forward, find_word_start_backward};
use ratatui::text::Line;
use std::cmp::Ordering;
use std::collections::HashMap;
use unicode_width::UnicodeWidthChar as _;

pub type MaybeLinks = Option<Vec<Link>>;

#[derive(Debug, Clone)]
enum YankText {
    Piece((String, MaybeLinks, (usize, usize))),
    Chunk((Vec<String>, MaybeLinks, (usize, usize))),
}

impl Default for YankText {
    fn default() -> Self {
        Self::Piece((String::new(), None, (0, 0)))
    }
}

impl ToString for YankText {
    fn to_string(&self) -> String {
        match self {
            Self::Piece((s, _, _)) => s.clone(),
            Self::Chunk((ss, _, _)) => ss.join("\n"),
        }
    }
}

/// A type to manage state of textarea.
///
/// [`TextArea::default`] creates an empty textarea. [`TextArea::new`] creates a textarea with given text lines.
/// [`TextArea::from`] creates a textarea from an iterator of lines. [`TextArea::input`] handles key input.
/// [`TextArea::widget`] builds a widget to render. And [`TextArea::lines`] returns line texts.
/// ```
/// use tuipaz_textarea::{TextArea, Input, Key};
///
/// let mut textarea = TextArea::default();
///
/// // Input 'a'
/// let input = Input { key: Key::Char('a'), ctrl: false, alt: false, shift: false };
/// textarea.input(input);
///
/// // Get widget to render.
/// let widget = textarea.widget();
///
/// // Get lines as String.
/// println!("Lines: {:?}", textarea.lines());
/// ```
#[derive(Clone, Debug)]
pub struct TextArea<'a> {
    lines: Vec<String>,
    block: Option<Block<'a>>,
    style: Style,
    cursor: (usize, usize), // 0-base
    pub links: HashMap<usize, Link>,
    pending_link: Option<(usize, usize)>,
    pub next_link_id: usize,
    pub new_link: bool,
    pub deleted_link_ids: Vec<usize>,
    pub copied_link_ids: HashMap<usize, usize>,
    tab_len: u8,
    hard_tab_indent: bool,
    history: History,
    cursor_line_style: Style,
    line_number_style: Option<Style>,
    pub(crate) viewport: Viewport,
    yank: YankText,
    search: Search,
    pub hop: Hop, // TODO! only pub for debug pursposes
    pub hop_pending: bool,
    pub hopping: bool,
    alignment: Alignment,
    pub(crate) placeholder: String,
    pub(crate) placeholder_style: Style,
    mask: Option<char>,
    selection_start: Option<(usize, usize)>,
    select_style: Style,
    cursor_style: Style,
    link_style: Style,
    max_col: u16,
}

pub struct TextAreaTheme {
    pub text: Color,
    pub select: Color,
    pub links: Color,
    pub main_heading: Color,
    pub main_heading_modifiers: Vec<Modifier>,
    pub sub_heading: Color,
    pub sub_heading_modifiers: Vec<Modifier>,
}

impl Default for TextAreaTheme {
    fn default() -> Self {
        Self {
            text: Color::default(),
            select: Color::Red,
            links: Color::Blue,
            main_heading: Color::Green,
            main_heading_modifiers: vec![Modifier::BOLD, Modifier::UNDERLINED],
            sub_heading: Color::Magenta,
            sub_heading_modifiers: vec![Modifier::ITALIC],
        }
    }
}

/// Convert any iterator whose elements can be converted into [`String`] into [`TextArea`]. Each [`String`] element is
/// handled as line. Ensure that the strings don't contain any newlines. This method is useful to create [`TextArea`]
/// from [`std::str::Lines`].
/// ```
/// use tuipaz_textarea::TextArea;
///
/// // From `String`
/// let text = "hello\nworld";
/// let textarea = TextArea::from(text.lines());
/// assert_eq!(textarea.lines(), ["hello", "world"]);
///
/// // From array of `&str`
/// let textarea = TextArea::from(["hello", "world"]);
/// assert_eq!(textarea.lines(), ["hello", "world"]);
///
/// // From slice of `&str`
/// let slice = &["hello", "world"];
/// let textarea = TextArea::from(slice.iter().copied());
/// assert_eq!(textarea.lines(), ["hello", "world"]);
/// ```
impl<'a, I> From<I> for TextArea<'a>
where
    I: IntoIterator,
    I::Item: Into<String>,
{
    fn from(i: I) -> Self {
        Self::new(
            i.into_iter().map(|s| s.into()).collect::<Vec<String>>(),
            HashMap::new(),
            140,
            TextAreaTheme::default(),
        )
    }
}

/// Collect line texts from iterator as [`TextArea`]. It is useful when creating a textarea with text read from a file.
/// [`Iterator::collect`] handles errors which may happen on reading each lines. The following example reads text from
/// a file efficiently line-by-line.
/// ```
/// use std::fs;
/// use std::io::{self, BufRead};
/// use std::path::Path;
/// use tuipaz_textarea::TextArea;
///
/// fn read_from_file<'a>(path: impl AsRef<Path>) -> io::Result<TextArea<'a>> {
///     let file = fs::File::open(path)?;
///     io::BufReader::new(file).lines().collect()
/// }
///
/// let textarea = read_from_file("README.md").unwrap();
/// assert!(!textarea.is_empty());
/// ```
impl<'a, S: Into<String>> FromIterator<S> for TextArea<'a> {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        iter.into()
    }
}

/// Create [`TextArea`] instance with empty text content.
/// ```
/// use tuipaz_textarea::TextArea;
///
/// let textarea = TextArea::default();
/// assert_eq!(textarea.lines(), [""]);
/// assert!(textarea.is_empty());
/// ```
impl<'a> Default for TextArea<'a> {
    fn default() -> Self {
        let theme = TextAreaTheme {
            text: Color::default(),
            select: Color::Magenta,
            links: Color::Blue,
            main_heading: Color::Green,
            main_heading_modifiers: vec![Modifier::BOLD, Modifier::UNDERLINED],
            sub_heading: Color::Magenta,
            sub_heading_modifiers: vec![Modifier::ITALIC],
        };

        Self::new(vec![String::new()], HashMap::new(), 140, theme)
    }
}

impl<'a> TextArea<'a> {
    /// Create [`TextArea`] instance with given lines. If you have value other than `Vec<String>`, [`TextArea::from`]
    /// may be more useful.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let lines = vec!["hello".to_string(), "...".to_string(), "goodbye".to_string()];
    /// let textarea = TextArea::new(lines);
    /// assert_eq!(textarea.lines(), ["hello", "...", "goodbye"]);
    /// ```
    pub fn new(
        mut lines: Vec<String>,
        links: HashMap<usize, Link>,
        max_col: u16,
        theme: TextAreaTheme,
    ) -> Self {
        if lines.is_empty() {
            lines.push(String::new());
        }

        let next_link_id = match links.keys().max() {
            Some(id) => id + 1,
            None => 0,
        };

        let style = Style::new().fg(theme.text);

        Self {
            lines,
            block: None,
            style,
            cursor: (0, 0),
            links,
            pending_link: None,
            next_link_id,
            new_link: false,
            deleted_link_ids: vec![],
            copied_link_ids: HashMap::new(),
            tab_len: 4,
            hard_tab_indent: false,
            history: History::new(50),
            cursor_line_style: Style::default(),
            line_number_style: None,
            viewport: Viewport::default(),
            yank: YankText::default(),
            search: Search::default(),
            hop: Hop::default(),
            hop_pending: false,
            hopping: false,
            alignment: Alignment::Left,
            placeholder: String::new(),
            placeholder_style: Style::default().fg(Color::DarkGray),
            mask: None,
            selection_start: None,
            select_style: Style::default().bg(theme.select),
            cursor_style: Style::default().add_modifier(Modifier::REVERSED),
            link_style: Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(theme.links),
            max_col,
        }
    }

    /// Handle a key input with default key mappings. For default key mappings, see the table in
    /// [the module document](./index.html).
    /// `crossterm`, `termion`, and `termwiz` features enable conversion from their own key event types into
    /// [`Input`] so this method can take the event values directly.
    /// This method returns if the input modified text contents or not in the textarea.
    /// ```ignore
    /// use tuipaz_textarea::{TextArea, Key, Input};
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// // Handle crossterm key events
    /// let event: crossterm::event::Event = ...;
    /// textarea.input(event);
    /// if let crossterm::event::Event::Key(key) = event {
    ///     textarea.input(key);
    /// }
    ///
    /// // Handle termion key events
    /// let event: termion::event::Event = ...;
    /// textarea.input(event);
    /// if let termion::event::Event::Key(key) = event {
    ///     textarea.input(key);
    /// }
    ///
    /// // Handle termwiz key events
    /// let event: termwiz::input::InputEvent = ...;
    /// textarea.input(event);
    /// if let termwiz::input::InputEvent::Key(key) = event {
    ///     textarea.input(key);
    /// }
    ///
    /// // Handle backend-agnostic key input
    /// let input = Input { key: Key::Char('a'), ctrl: false, alt: false, shift: false };
    /// let modified = textarea.input(input);
    /// assert!(modified);
    /// ```
    pub fn input(&mut self, input: impl Into<Input>) -> bool {
        let input = input.into();
        let modified = match input {
            Input {
                key: Key::Char('m'),
                ctrl: true,
                alt: false,
                ..
            }
            | Input {
                key: Key::Char('\n' | '\r'),
                ctrl: false,
                alt: false,
                ..
            }
            | Input {
                key: Key::Enter, ..
            } => {
                self.insert_newline();
                true
            }
            Input {
                key: Key::Char(c),
                ctrl: false,
                alt: false,
                ..
            } => {
                self.insert_char(c);
                true
            }
            Input {
                key: Key::Tab,
                ctrl: false,
                alt: false,
                ..
            } => self.insert_tab(),
            Input {
                key: Key::Char('h'),
                ctrl: true,
                alt: false,
                ..
            }
            | Input {
                key: Key::Backspace,
                ctrl: false,
                alt: false,
                ..
            } => self.delete_char(),
            Input {
                key: Key::Char('d'),
                ctrl: true,
                alt: false,
                ..
            }
            | Input {
                key: Key::Delete,
                ctrl: false,
                alt: false,
                ..
            } => self.delete_next_char(),
            Input {
                key: Key::Char('k'),
                ctrl: true,
                alt: false,
                ..
            } => self.delete_line_by_end(),
            Input {
                key: Key::Char('j'),
                ctrl: true,
                alt: false,
                ..
            } => self.delete_line_by_head(),
            Input {
                key: Key::Char('w'),
                ctrl: true,
                alt: false,
                ..
            }
            | Input {
                key: Key::Char('h'),
                ctrl: false,
                alt: true,
                ..
            }
            | Input {
                key: Key::Backspace,
                ctrl: false,
                alt: true,
                ..
            } => self.delete_word(),
            Input {
                key: Key::Delete,
                ctrl: false,
                alt: true,
                ..
            }
            | Input {
                key: Key::Char('d'),
                ctrl: false,
                alt: true,
                ..
            } => self.delete_next_word(),
            Input {
                key: Key::Char('n'),
                ctrl: true,
                alt: false,
                shift,
            }
            | Input {
                key: Key::Down,
                ctrl: false,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::Down, shift);
                false
            }
            Input {
                key: Key::Char('p'),
                ctrl: true,
                alt: false,
                shift,
            }
            | Input {
                key: Key::Up,
                ctrl: false,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::Up, shift);
                false
            }
            Input {
                key: Key::Char('f'),
                ctrl: true,
                alt: false,
                shift,
            }
            | Input {
                key: Key::Right,
                ctrl: false,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::Forward, shift);
                false
            }
            Input {
                key: Key::Char('b'),
                ctrl: true,
                alt: false,
                shift,
            }
            | Input {
                key: Key::Left,
                ctrl: false,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::Back, shift);
                false
            }
            Input {
                key: Key::Char('a'),
                ctrl: true,
                alt: false,
                shift,
            }
            | Input {
                key: Key::Home,
                shift,
                ..
            }
            | Input {
                key: Key::Left | Key::Char('b'),
                ctrl: true,
                alt: true,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::Head, shift);
                false
            }
            Input {
                key: Key::Char('e'),
                ctrl: true,
                alt: false,
                shift,
            }
            | Input {
                key: Key::End,
                shift,
                ..
            }
            | Input {
                key: Key::Right | Key::Char('f'),
                ctrl: true,
                alt: true,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::End, shift);
                false
            }
            Input {
                key: Key::Char('<'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Up | Key::Char('p'),
                ctrl: true,
                alt: true,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::Top, shift);
                false
            }
            Input {
                key: Key::Char('>'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Down | Key::Char('n'),
                ctrl: true,
                alt: true,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::Bottom, shift);
                false
            }
            Input {
                key: Key::Char('f'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Right,
                ctrl: true,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::WordForward, shift);
                false
            }
            Input {
                key: Key::Char('b'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Left,
                ctrl: true,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::WordBack, shift);
                false
            }
            Input {
                key: Key::Char(']'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Char('n'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Down,
                ctrl: true,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::ParagraphForward, shift);
                false
            }
            Input {
                key: Key::Char('['),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Char('p'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::Up,
                ctrl: true,
                alt: false,
                shift,
            } => {
                self.move_cursor_with_shift(CursorMove::ParagraphBack, shift);
                false
            }
            Input {
                key: Key::Char('u'),
                ctrl: true,
                alt: false,
                ..
            } => self.undo(),
            Input {
                key: Key::Char('r'),
                ctrl: true,
                alt: false,
                ..
            } => self.redo(),
            Input {
                key: Key::Char('y'),
                ctrl: true,
                alt: false,
                ..
            }
            | Input {
                key: Key::Paste, ..
            } => self.paste(),
            Input {
                key: Key::Char('x'),
                ctrl: true,
                alt: false,
                ..
            }
            | Input { key: Key::Cut, .. } => self.cut(),
            Input {
                key: Key::Char('c'),
                ctrl: true,
                alt: false,
                ..
            }
            | Input { key: Key::Copy, .. } => {
                self.copy();
                false
            }
            Input {
                key: Key::Char('v'),
                ctrl: true,
                alt: false,
                shift,
            }
            | Input {
                key: Key::PageDown,
                shift,
                ..
            } => {
                self.scroll_with_shift(Scrolling::PageDown, shift);
                false
            }
            Input {
                key: Key::Char('v'),
                ctrl: false,
                alt: true,
                shift,
            }
            | Input {
                key: Key::PageUp,
                shift,
                ..
            } => {
                self.scroll_with_shift(Scrolling::PageUp, shift);
                false
            }
            Input {
                key: Key::MouseScrollDown,
                shift,
                ..
            } => {
                self.scroll_with_shift((1, 0).into(), shift);
                false
            }
            Input {
                key: Key::MouseScrollUp,
                shift,
                ..
            } => {
                self.scroll_with_shift((-1, 0).into(), shift);
                false
            }
            _ => false,
        };

        // Check invariants
        debug_assert!(!self.lines.is_empty(), "no line after {:?}", input);
        let (r, c) = self.cursor;
        debug_assert!(
            self.lines.len() > r,
            "cursor {:?} exceeds max lines {} after {:?}",
            self.cursor,
            self.lines.len(),
            input,
        );
        debug_assert!(
            self.lines[r].chars().count() >= c,
            "cursor {:?} exceeds max col {} at line {:?} after {:?}",
            self.cursor,
            self.lines[r].chars().count(),
            self.lines[r],
            input,
        );

        modified
    }

    /// Handle a key input without default key mappings. This method handles only
    ///
    /// - Single character input without modifier keys
    /// - Tab
    /// - Enter
    /// - Backspace
    /// - Delete
    ///
    /// This method returns if the input modified text contents or not in the textarea.
    ///
    /// This method is useful when you want to define your own key mappings and don't want default key mappings.
    /// See 'Define your own key mappings' section in [the module document](./index.html).
    pub fn input_without_shortcuts(&mut self, input: impl Into<Input>) -> bool {
        match input.into() {
            Input {
                key: Key::Char(c),
                ctrl: false,
                alt: false,
                ..
            } => {
                self.insert_char(c);
                true
            }
            Input {
                key: Key::Tab,
                ctrl: false,
                alt: false,
                ..
            } => self.insert_tab(),
            Input {
                key: Key::Backspace,
                ..
            } => self.delete_char(),
            Input {
                key: Key::Delete, ..
            } => self.delete_next_char(),
            Input {
                key: Key::Enter, ..
            } => {
                self.insert_newline();
                true
            }
            Input {
                key: Key::MouseScrollDown,
                ..
            } => {
                self.scroll((1, 0));
                false
            }
            Input {
                key: Key::MouseScrollUp,
                ..
            } => {
                self.scroll((-1, 0));
                false
            }
            _ => false,
        }
    }

    fn push_history(&mut self, kind: EditKind, before: Pos, after_offset: usize) {
        let (row, col) = self.cursor;
        let after = Pos::new(row, col, after_offset);
        let edit = Edit::new(kind, before, after);
        self.history.push(edit);
    }

    fn get_current_line_len(&self) -> u16 {
        self.lines[self.cursor.0].len() as u16
    }

    fn get_prev_line_len(&self) -> u16 {
        self.lines[self.cursor.0 - 1].len() as u16
    }

    fn check_current_row_overhang(&self) -> bool {
        self.get_current_line_len() >= self.max_col - 1
    }

    fn check_prev_row_space(&self) -> bool {
        self.get_prev_line_len() < self.max_col - 1
    }

    /// Insert a single character at current cursor position.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.insert_char('a');
    /// assert_eq!(textarea.lines(), ["a"]);
    /// ```
    pub fn insert_char(&mut self, c: char) {
        if c == '\n' || c == '\r' {
            self.insert_newline();
            return;
        } else if c == '[' && !self.lines[self.cursor.0].starts_with('#') {
            self.init_link();
        } else if c == ']' && !self.lines[self.cursor.0].starts_with('#') {
            self.insert_link();
        } else if self.check_current_row_overhang() {
            self.shift_lines_after_insert();
        }

        let (row, col) = self.cursor;
        self.shift_links_same_row(row, (col, col + 1));

        let line = &mut self.lines[row];
        let i = line
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        line.insert(i, c);
        self.cursor.1 += 1;
        self.push_history(
            EditKind::InsertChar((c, None)),
            Pos::new(row, col, i),
            i + c.len_utf8(),
        );
    }

    /// Insert a string at current cursor position. This method returns if some text was inserted or not in the textarea.
    /// Both `\n` and `\r\n` are recognized as newlines but `\r` isn't.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.insert_str("hello");
    /// assert_eq!(textarea.lines(), ["hello"]);
    ///
    /// textarea.insert_str(", world\ngoodbye, world");
    /// assert_eq!(textarea.lines(), ["hello, world", "goodbye, world"]);
    /// ```
    pub fn insert_str<S: AsRef<str>>(&mut self, s: S, insert_links: MaybeLinks) -> bool {
        let modified = self.delete_selection(false);
        let mut lines: Vec<_> = s
            .as_ref()
            .split('\n')
            .map(|s| s.strip_suffix('\r').unwrap_or(s).to_string())
            .collect();

        info!("insert_str:: lines.len(): {}", lines.len());
        match lines.len() {
            0 => modified,
            1 => self.insert_piece(lines.remove(0), insert_links),
            _ => self.insert_chunk(lines, insert_links),
        }
    }

    fn insert_chunk(&mut self, chunk: Vec<String>, insert_links: MaybeLinks) -> bool {
        debug_assert!(chunk.len() > 1, "Chunk size must be > 1: {:?}", chunk);

        let (row, col) = self.cursor;
        let line = &mut self.lines[row];
        let i = line
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let before = Pos::new(row, col, i);

        let clen = chunk.len();
        let (new_row, new_col) = (row + clen - 1, chunk[clen - 1].chars().count());
        self.cursor = (new_row, new_col);
        self.shift_links_after_insert((row, col), (new_row, new_col));

        let end_offset = chunk.last().unwrap().len();

        let inserted_links =
            insert_links.map(|il| il.into_iter().map(|l| l.id).collect::<Vec<usize>>());

        let mut edit = EditKind::InsertChunk((chunk, inserted_links));
        edit.apply(
            &mut self.lines,
            &mut self.links,
            &before,
            &Pos::new(new_row, new_col, end_offset),
        );

        self.push_history(edit, before, end_offset);
        true
    }

    fn insert_piece(&mut self, s: String, insert_links: MaybeLinks) -> bool {
        info!("INSERT PIECE");
        if s.is_empty() {
            return false;
        }

        let (row, col) = self.cursor;
        let line = &mut self.lines[row];
        debug_assert!(
            !s.contains('\n'),
            "string given to TextArea::insert_piece must not contain newline: {:?}",
            line,
        );

        let line_len = line.len();
        let s_len = s.len();
        let i = line
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line_len);
        let end_offset = i + s_len;

        let overhang = (line_len + s_len) as i32 - self.max_col as i32;

        info!(
            "insert_piece\ni: {:?}\nend_offset: {:?}\noverhang: {}\nself.max_col: {}",
            i, end_offset, overhang, self.max_col
        );

        line.insert_str(i, &s);
        info!("line: {}", &line);
        info!(
            "inserting piece\nstart: (row, col): ({}, {})
            \nend: (row, col): ({}, {})",
            row, col, row, self.cursor.1,
        );

        self.cursor.1 += s_len;
        let start_cursor = self.cursor;

        self.shift_links_after_insert((row, col), (row, self.cursor.1));
        info!("self.cursor after move: {:?}", self.cursor);
        if overhang >= 0 {
            let max_col = self.max_col as usize;
            self.shift_lines_after_insert();
            self.cursor = match start_cursor.1 >= max_col {
                true => (start_cursor.0 + 1, end_offset - max_col),
                false => (start_cursor.0, start_cursor.1 + s_len - 1),
            };
        }

        let inserted_links =
            insert_links.map(|il| il.into_iter().map(|l| l.id).collect::<Vec<usize>>());

        self.push_history(
            EditKind::InsertStr((s, inserted_links)),
            Pos::new(row, col, i),
            end_offset,
        );
        true
    }

    fn delete_range(&mut self, start: Pos, end: Pos, deleted_links: MaybeLinks, should_yank: bool) {
        info!("INSIDE delete_range");
        self.cursor = (start.row, start.col);

        let link_ids = deleted_links
            .as_ref()
            .map(|dl| dl.iter().map(|l| l.id).collect::<Vec<usize>>());

        if start.row == end.row {
            let removed = self.lines[start.row]
                .drain(start.offset..end.offset)
                .as_str()
                .to_string();
            if should_yank {
                self.yank =
                    YankText::Piece((removed.clone(), deleted_links, (start.row, start.col)));
            }
            self.push_history(EditKind::DeleteStr((removed, link_ids)), end, start.offset);
            return;
        }

        let mut deleted = vec![self.lines[start.row]
            .drain(start.offset..)
            .as_str()
            .to_string()];
        deleted.extend(self.lines.drain(start.row + 1..end.row));
        if start.row + 1 < self.lines.len() {
            let mut last_line = self.lines.remove(start.row + 1);
            self.lines[start.row].push_str(&last_line[end.offset..]);
            last_line.truncate(end.offset);
            deleted.push(last_line);
        }

        if should_yank {
            self.yank = YankText::Chunk((deleted.clone(), deleted_links, (start.row, start.col)));
        }

        let edit = if deleted.len() == 1 {
            EditKind::DeleteStr((deleted.remove(0), link_ids))
        } else {
            EditKind::DeleteChunk((deleted, link_ids))
        };
        self.push_history(edit, end, start.offset);
    }

    /// Delete a string from the current cursor position. The `chars` parameter means number of characters, not a byte
    /// length of the string. Newlines at the end of lines are counted in the number. This method returns if some text
    /// was deleted or not.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["🐱🐶🐰🐮"]);
    /// textarea.move_cursor(CursorMove::Forward);
    ///
    /// textarea.delete_str(2);
    /// assert_eq!(textarea.lines(), ["🐱🐮"]);
    ///
    /// let mut textarea = TextArea::from(["🐱", "🐶", "🐰", "🐮"]);
    /// textarea.move_cursor(CursorMove::Down);
    ///
    /// textarea.delete_str(4); // Deletes 🐶, \n, 🐰, \n
    /// assert_eq!(textarea.lines(), ["🐱", "🐮"]);
    /// ```
    pub fn delete_str(&mut self, chars: usize) -> bool {
        info!("INSIDE delete_str");
        if self.delete_selection(false) {
            return true;
        }
        if chars == 0 {
            return false;
        }

        let (start_row, start_col) = self.cursor;

        let mut remaining = chars;
        let mut find_end = move |line: &str| {
            let mut col = 0usize;
            for (i, _) in line.char_indices() {
                if remaining == 0 {
                    return Some((i, col));
                }
                col += 1;
                remaining -= 1;
            }
            if remaining == 0 {
                Some((line.len(), col))
            } else {
                remaining -= 1;
                None
            }
        };

        let line = &self.lines[start_row];
        let start_offset = {
            line.char_indices()
                .nth(start_col)
                .map(|(i, _)| i)
                .unwrap_or(line.len())
        };

        // First line
        if let Some((offset_delta, col_delta)) = find_end(&line[start_offset..]) {
            let end_offset = start_offset + offset_delta;
            let end_col = start_col + col_delta;
            let removed = self.lines[start_row]
                .drain(start_offset..end_offset)
                .as_str()
                .to_string();

            let start = Pos {
                row: start_row,
                col: start_col,
                offset: start_offset,
            };

            let end = Pos {
                row: start_row,
                col: end_col,
                offset: end_offset,
            };

            let l = self
                .links
                .iter()
                .filter(|(_, l)| Self::link_in_range(l, &start, &end))
                .map(|(id, _)| {
                    let l = self.links.get(id).expect("deleted link will exist");

                    let row_offset = l.row - start.row;
                    let (start_col_offset, end_col_offset) = if row_offset == 0 {
                        (l.start_col - start.col, l.end_col - start.col)
                    } else {
                        (l.start_col, l.end_col)
                    };

                    Link {
                        id: *id,
                        row: row_offset,
                        start_col: start_col_offset,
                        end_col: end_col_offset,
                        edited: false,
                        deleted: false,
                    }
                })
                .collect::<Vec<Link>>();

            let deleted_links = match l.is_empty() {
                true => None,
                false => Some(l),
            };

            self.yank = YankText::Piece((removed.clone(), deleted_links, (start.row, start.col)));
            self.push_history(
                EditKind::DeleteStr((removed, None)),
                Pos::new(start_row, end_col, end_offset),
                start_offset,
            );
            return true;
        }

        let mut r = start_row + 1;
        let mut offset = 0;
        let mut col = 0;

        while r < self.lines.len() {
            let line = &self.lines[r];
            if let Some((o, c)) = find_end(line) {
                offset = o;
                col = c;
                break;
            }
            r += 1;
        }

        let start = Pos::new(start_row, start_col, start_offset);
        let end = Pos::new(r, col, offset);
        self.delete_range(start, end, None, true);
        true
    }

    fn delete_piece(&mut self, col: usize, chars: usize) -> bool {
        info!("INSIDE delete_piece");
        if chars == 0 {
            return false;
        }

        #[inline]
        fn bytes_and_chars(claimed: usize, s: &str) -> (usize, usize) {
            // Note: `claimed` may be larger than characters in `s` (e.g. usize::MAX)
            let mut last_col = 0;
            for (col, (bytes, _)) in s.char_indices().enumerate() {
                if col == claimed {
                    return (bytes, claimed);
                }
                last_col = col;
            }
            (s.len(), last_col + 1)
        }

        let (row, _) = self.cursor;
        let line = &mut self.lines[row];
        if let Some((i, _)) = line.char_indices().nth(col) {
            let (bytes, chars) = bytes_and_chars(chars, &line[i..]);
            let removed = line.drain(i..i + bytes).as_str().to_string();
            let line_empty = line.is_empty();

            let deleted_links = self.delete_links_in_range((row, col), (row, col + chars));

            let link_ids = deleted_links
                .as_ref()
                .map(|dl| dl.iter().map(|l| l.id).collect::<Vec<usize>>());

            let start_col = match line_empty {
                true => col,
                false => col + chars,
            };

            self.shift_links_after_delete((row, start_col), (row, col), 0);

            self.cursor = (row, col);
            self.push_history(
                EditKind::DeleteStr((removed.clone(), link_ids)),
                Pos::new(row, col + chars, i + bytes),
                i,
            );
            self.yank = YankText::Piece((removed, deleted_links, (row, start_col)));
            true
        } else {
            false
        }
    }

    /// Insert a tab at current cursor position. Note that this method does nothing when the tab length is 0. This
    /// method returns if a tab string was inserted or not in the textarea.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["hi"]);
    ///
    /// textarea.move_cursor(CursorMove::End); // Move to the end of line
    ///
    /// textarea.insert_tab();
    /// assert_eq!(textarea.lines(), ["hi  "]);
    /// textarea.insert_tab();
    /// assert_eq!(textarea.lines(), ["hi      "]);
    /// ```
    pub fn insert_tab(&mut self) -> bool {
        let modified = self.delete_selection(false);
        if self.tab_len == 0 {
            return modified;
        }

        if self.hard_tab_indent {
            self.insert_char('\t');
            return true;
        }

        let (row, col) = self.cursor;
        let width: usize = self.lines[row]
            .chars()
            .take(col)
            .map(|c| c.width().unwrap_or(0))
            .sum();
        let len = self.tab_len - (width % self.tab_len as usize) as u8;
        let edited_links = None;
        self.insert_piece(spaces(len).to_string(), edited_links)
    }

    fn shift_lines_after_insert(&mut self) {
        info!("shift_lines_after_insert");
        let (start_row, start_col) = self.cursor;
        let mut insert_at_end_of_line = false;
        let mut word_offset = 0;
        let max_row = self.lines.len();
        info!("shift_lines_after_insert::max_row: {}", max_row);

        while self.cursor.0 < max_row && self.check_current_row_overhang() {
            info!(
                "shift_lines_after_insert::current line len {}",
                self.lines[self.cursor.0].len()
            );
            info!(
                "shift_lines_after_insert::current_row BEFORE: {}",
                self.cursor.0
            );
            (word_offset, insert_at_end_of_line) = self.shift_overhang_newline();
            info!(
                "shift_lines_after_insert::current_row AFTER: {}",
                self.cursor.0
            );
            info!("shift_lines_after_insert::word_offset: {:?}", word_offset);
        }

        info!(
            "shift_lines_after_insert::insert_at_end_of_line: {}",
            insert_at_end_of_line
        );
        if insert_at_end_of_line {
            self.cursor = (start_row + 1, word_offset);
        } else {
            let max_line_col = self.lines[start_row].len() - 1;
            info!(
                "shift_lines_after_insert::max_line_col <= start_col: {}",
                start_col >= max_line_col
            );
            self.cursor = match start_col >= max_line_col {
                true => (start_row + 1, 0),
                false => (start_row, start_col),
            };
        }
        info!(
            "shift_lines_after_insert:: FINAL self.cursor: {:?}",
            self.cursor
        );
    }

    fn shift_overhang_newline(&mut self) -> (usize, bool) {
        info!("shift_overhang_newline");
        let insert_at_end_of_line = self.cursor.1 as u16 >= self.max_col - 1;
        let max_col = (self.max_col - 1) as usize;
        let (start_row, start_col) = (self.cursor.0 as u16, self.cursor.1 as u16);

        let row = self.cursor.0;
        let current_line = &self.lines[row];
        let mut word_start =
            find_word_start_backward(current_line, max_col).expect("Should find word_start");
        info!("shift_overhang_newline::initial word_start {}", word_start);

        // If word_start is in a link move to start_col of that link
        // and shift link to 0th col of next line.
        if let Some(id) = self.in_link((row, word_start)) {
            let l = self
                .links
                .get_mut(&id)
                .expect("Link should exist at with start_col == word_start");

            info!("shift_overhang_newline::word_start in link BEFORE: {:?}", l);
            word_start = l.start_col;

            let end_offset = l.end_col - l.start_col;
            l.start_col = 0;
            l.end_col = end_offset;
            l.row += 1;
            l.edited = true;
            info!("shift_overhang_newline::word_start in link AFTER: {:?}", l);
        } else if self.lines[row].chars().nth(max_col) == Some(' ') {
            info!("shift_overhang_newline::max_col - 1 char is space");
            word_start = max_col;
        }
        // If there are links after word_start, shift them to next line
        if let Some(id_vec) = self.links_in_row_after_cursor((row, word_start)) {
            for id in id_vec {
                let l = self
                    .links
                    .get_mut(&id)
                    .expect("Link should exist at with start_col == word_start");

                info!("shift_overhang_newline::link_after_cursor BEFORE: {:?}", l);
                let overhang = l.start_col - word_start;
                info!("shift_overhang_newline::link overhang: {}", overhang);
                let link_len = l.end_col - l.start_col;
                l.start_col = overhang;
                l.end_col = l.start_col + link_len;
                l.row += 1;
                l.edited = true;
                info!("shift_overhang_newline::link_after_cursor AFTER: {:?}", l);
            }
        }

        // String to push to next line
        let overhang_str = self.lines[row].drain(word_start..).as_str().to_owned();
        let word_offset = overhang_str.len();
        info!("shift_overhang_newline::overhang_str: {:?}", overhang_str);

        if self.lines.len() <= row + 1 {
            self.move_cursor(CursorMove::End);
            info!("shift_overhang_newline:: inserting newline");
            self.insert_newline();
        } else {
            info!("shift_overhang_newline:: NOT inserting newline");
            self.cursor = (row + 1, 0);
        }

        self.prepend_next_line(overhang_str, self.cursor);

        info!(
            "shift_overhang_newline::insert_at_end_of_line: {}",
            insert_at_end_of_line
        );
        // Insert NOT at end of line -> cursor jumps back to start pos
        if !insert_at_end_of_line {
            self.move_cursor(CursorMove::Jump(start_row, start_col));
        }

        (word_offset, insert_at_end_of_line)
    }

    fn prepend_next_line(&mut self, s: String, insert_pos: (usize, usize)) -> bool {
        info!("prepend_next_line");
        if s.is_empty() {
            return false;
        }

        let (row, col) = self.cursor;
        let line = &mut self.lines[row];
        debug_assert!(
            !s.contains('\n'),
            "string given to TextArea::prepend_next_line must not contain newline: {:?}",
            line,
        );
        info!(
            "prepend_next_line::inserting '{}'...\n...into line: '{}'",
            s, line
        );

        let i = line
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        line.insert_str(i, &s);

        info!(
            "prepend_next_line::self.cursor before s.chars().count(): {:?}",
            self.cursor
        );
        self.cursor.1 += s.chars().count();
        info!(
            "prepend_next_line::self.cursor after s.chars().count(): {:?}",
            self.cursor
        );
        self.shift_links_after_insert((row, col), (row, self.cursor.1));
        info!(
            "prepend_next_line::self.cursor after shift_links_after_insert: {:?}",
            self.cursor
        );
        true
    }

    /// Insert a newline at current cursor position.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["hi"]);
    ///
    /// textarea.move_cursor(CursorMove::Forward);
    /// textarea.insert_newline();
    /// assert_eq!(textarea.lines(), ["h", "i"]);
    /// ```
    pub fn insert_newline(&mut self) {
        info!("insert_newline");
        self.delete_selection(false);

        let (row, col) = self.cursor;

        let line = &mut self.lines[row];
        let offset = line
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let next_line = line[offset..].to_string();
        line.truncate(offset);

        self.lines.insert(row + 1, next_line);
        self.shift_links_newline(self.cursor);

        self.cursor = (row + 1, 0);
        self.push_history(EditKind::InsertNewline, Pos::new(row, col, offset), 0);
    }

    /// Delete a newline from **head** of current cursor line. This method returns if a newline was deleted or not in
    /// the textarea. When some text is selected, it is deleted instead.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["hello", "world"]);
    ///
    /// textarea.move_cursor(CursorMove::Down);
    /// textarea.delete_newline();
    /// assert_eq!(textarea.lines(), ["helloworld"]);
    /// ```
    pub fn delete_newline(&mut self) -> bool {
        info!("Inside delete_newline");
        if self.delete_selection(false) {
            return true;
        }

        let (row, _) = self.cursor;
        if row == 0 {
            return false;
        }

        let line_end = self.lines[row].len();
        let prev_line_end = self.lines[row - 1].len();
        let mut to_drain = self.max_col as usize - prev_line_end - 1;
        info!("delete_newline::line_end: {:?}", line_end);
        info!("delete_newline::prev_line_end: {:?}", prev_line_end);
        let line = if prev_line_end == 0
            || line_end == 0
            || line_end + prev_line_end < self.max_col as usize
        {
            info!("removing whole row");
            self.lines.remove(row)
        } else {
            to_drain = find_word_start_backward(&self.lines[row], to_drain).unwrap_or(to_drain);
            to_drain = std::cmp::min(to_drain, line_end);
            self.lines[row].drain(..to_drain).collect::<String>()
        };

        info!("delete_newline::to_drain: {:?}", to_drain);
        self.shift_links_after_delete((row, 0), (row - 1, prev_line_end), to_drain);

        info!("delete_newline::line to push to prev: {:?}", line);
        let prev_line = &mut self.lines[row - 1];
        self.cursor = (row - 1, prev_line.chars().count());
        prev_line.push_str(&line);
        self.push_history(EditKind::DeleteNewline, Pos::new(row, 0, 0), prev_line_end);
        true
    }

    pub fn delete_line(&mut self, shift_up: bool) -> bool {
        let (row, _) = self.cursor;

        let line = self.lines.remove(row);

        let deleted_links = self.delete_links_in_range((row, 0), (row, std::usize::MAX));
        self.shift_links_after_delete((row + 1, 0), (row, 0), 0);

        let link_ids = deleted_links.map(|dl| dl.iter().map(|yl| yl.id).collect::<Vec<usize>>());

        self.push_history(
            EditKind::DeleteLine((line, link_ids)),
            Pos::new(row + 1, 0, 0),
            0,
        );

        if row == 0 && self.lines.is_empty() {
            self.lines.push("".to_string());
            self.cursor = (0, 0);
        } else if row == self.lines.len() {
            self.cursor.0 = row - 1;
        } else {
            self.cursor.0 = match shift_up {
                true => self.cursor.0.saturating_sub(1),
                false => self.cursor.0,
            };
        }

        true
    }

    /// Delete one character before cursor. When the cursor is at head of line, the newline before the cursor will be
    /// removed. This method returns if some text was deleted or not in the textarea. When some text is selected, it is
    /// deleted instead.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["abc"]);
    ///
    /// textarea.move_cursor(CursorMove::Forward);
    /// textarea.delete_char();
    /// assert_eq!(textarea.lines(), ["bc"]);
    /// ```
    pub fn delete_char(&mut self) -> bool {
        info!("INSIDE delete_char");
        if self.delete_selection(false) {
            return true;
        }

        let (row, col) = self.cursor;
        info!("delete_char::self.cursor BEFORE: {:?}", (row, col));

        let delete_pos = match (row > 0, col > 0) {
            (true, false) => {
                let row_up = row - 1;
                let end_col_up = self.lines[row_up].len();
                (row_up, end_col_up)
            }
            (false, false) => (0, 0),
            _ => (row, col - 1),
        };

        let mut links = vec![];
        if let Some(id) = self.in_link(delete_pos) {
            links.push(self.delete_link(id));
        }

        let link_ids = match links.is_empty() {
            true => None,
            false => Some(links),
        };

        if col == 0 {
            info!("delete_char -> delete_newline");
            return self.delete_newline();
        }

        info!(
            "delete_char::self.cursor AFTER shift_lines_after_delete: {:?}",
            (row, col)
        );
        self.shift_links_after_delete((row, col), delete_pos, 0);

        let line = &mut self.lines[row];
        if let Some((offset, c)) = line.char_indices().nth(col - 1) {
            info!("delete_char::inside if let Some ...etc ");
            line.remove(offset);
            self.cursor.1 -= 1;
            self.push_history(
                EditKind::DeleteChar((c, link_ids)),
                Pos::new(row, col, offset + c.len_utf8()),
                offset,
            );
            true
        } else {
            false
        }
    }

    /// Delete one character next to cursor. When the cursor is at end of line, the newline next to the cursor will be
    /// removed. This method returns if a character was deleted or not in the textarea.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["abc"]);
    ///
    /// textarea.move_cursor(CursorMove::Forward);
    /// textarea.delete_next_char();
    /// assert_eq!(textarea.lines(), ["ac"]);
    /// ```
    pub fn delete_next_char(&mut self) -> bool {
        info!("DELETE_NEXT_CHAR");
        if self.delete_selection(false) {
            info!("delete_next_char::delete_selection");
            return true;
        }

        let before = self.cursor;

        self.move_cursor_with_shift(CursorMove::Forward, false);
        if before == self.cursor {
            return false; // Cursor didn't move, meant no character at next of cursor.
        }

        info!("delete_next_char::no character at next of cursor");
        self.delete_char()
    }

    /// Delete string from cursor to end of the line. When the cursor is at end of line, the newline next to the cursor
    /// is removed. This method returns if some text was deleted or not in the textarea.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["abcde"]);
    ///
    /// // Move to 'c'
    /// textarea.move_cursor(CursorMove::Forward);
    /// textarea.move_cursor(CursorMove::Forward);
    ///
    /// textarea.delete_line_by_end();
    /// assert_eq!(textarea.lines(), ["ab"]);
    /// ```
    pub fn delete_line_by_end(&mut self) -> bool {
        info!("DELETE_LINE_BY_END");
        if self.delete_selection(false) {
            info!("delete_line_by_end::delete_selection");
            return true;
        }
        let (_, col) = self.cursor;
        if self.delete_piece(col, usize::MAX) {
            info!("delete_line_by_end::delete_piece");
            return true;
        }

        info!("delete_line_by_end::delete_next_char");
        self.delete_next_char() // At the end of the line. Try to delete next line
    }

    /// Delete string from cursor to head of the line. When the cursor is at head of line, the newline before the cursor
    /// will be removed. This method returns if some text was deleted or not in the textarea.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["abcde"]);
    ///
    /// // Move to 'c'
    /// textarea.move_cursor(CursorMove::Forward);
    /// textarea.move_cursor(CursorMove::Forward);
    ///
    /// textarea.delete_line_by_head();
    /// assert_eq!(textarea.lines(), ["cde"]);
    /// ```
    pub fn delete_line_by_head(&mut self) -> bool {
        info!("DELETE_LINE_BY_HEAD");
        if self.delete_selection(false) {
            info!("delete_line_by_head::delete_selection");
            return true;
        }
        if self.delete_piece(0, self.cursor.1) {
            info!("delete_line_by_head::delete_piece");
            return true;
        }
        self.delete_newline()
    }

    /// Delete a word before cursor. Word boundary appears at spaces, punctuations, and others. For example `fn foo(a)`
    /// consists of words `fn`, `foo`, `(`, `a`, `)`. When the cursor is at head of line, the newline before the cursor
    /// will be removed.
    ///
    /// This method returns if some text was deleted or not in the textarea.
    ///
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["aaa bbb ccc"]);
    ///
    /// textarea.move_cursor(CursorMove::End);
    ///
    /// textarea.delete_word();
    /// assert_eq!(textarea.lines(), ["aaa bbb "]);
    /// textarea.delete_word();
    /// assert_eq!(textarea.lines(), ["aaa "]);
    /// ```
    pub fn delete_word(&mut self) -> bool {
        info!("DELETE_WORD");
        if self.delete_selection(false) {
            return true;
        }

        let (r, start_col) = self.cursor;
        if let Some(end_col) = find_word_start_backward(&self.lines[r], start_col) {
            self.delete_piece(end_col, start_col - end_col)
        } else if start_col > 0 {
            self.delete_piece(0, start_col)
        } else {
            self.delete_newline()
        }
    }

    /// Delete a word next to cursor. Word boundary appears at spaces, punctuations, and others. For example `fn foo(a)`
    /// consists of words `fn`, `foo`, `(`, `a`, `)`. When the cursor is at end of line, the newline next to the cursor
    /// will be removed.
    ///
    /// This method returns if some text was deleted or not in the textarea.
    ///
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::from(["aaa bbb ccc"]);
    ///
    /// textarea.delete_next_word();
    /// assert_eq!(textarea.lines(), [" bbb ccc"]);
    /// textarea.delete_next_word();
    /// assert_eq!(textarea.lines(), [" ccc"]);
    /// ```
    pub fn delete_next_word(&mut self) -> bool {
        info!("DELETE_NEXT_WORD");
        if self.delete_selection(false) {
            return true;
        }

        let (r, start_col) = self.cursor;
        let line = &self.lines[r];
        if let Some(end_col) = find_word_end_forward(line, start_col) {
            self.delete_piece(start_col, end_col - start_col)
        } else {
            let end_col = line.chars().count();
            if start_col < end_col {
                self.delete_piece(start_col, end_col - start_col)
            } else if r + 1 < self.lines.len() {
                self.cursor = (r + 1, 0);
                self.delete_newline()
            } else {
                false
            }
        }
    }

    /// Paste a string previously deleted by [`TextArea::delete_line_by_head`], [`TextArea::delete_line_by_end`],
    /// [`TextArea::delete_word`], [`TextArea::delete_next_word`]. This method returns if some text was inserted or not
    /// in the textarea.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["aaa bbb ccc"]);
    ///
    /// textarea.delete_next_word();
    /// textarea.move_cursor(CursorMove::End);
    /// textarea.paste();
    /// assert_eq!(textarea.lines(), [" bbb cccaaa"]);
    /// ```
    pub fn paste(&mut self) -> bool {
        self.delete_selection(false);
        let (row, col) = self.cursor;
        match self.yank.clone() {
            YankText::Piece((s, l, _)) => {
                let mut link_copies = vec![];
                if let Some(yanked_links) = l {
                    for yanked_link in yanked_links.iter() {
                        let link = self
                            .links
                            .get_mut(&yanked_link.id)
                            .expect("Link to paste should be in links hashmap");

                        match link.deleted {
                            true => {
                                link.deleted = false;
                                link.edited = true;
                                link.row = row + yanked_link.row;
                                link.start_col = col + yanked_link.start_col;
                                link.end_col = col + yanked_link.end_col;
                            }
                            false => {
                                let mut copied_link = Link::new(
                                    self.next_link_id,
                                    row,
                                    col + yanked_link.start_col,
                                    col + yanked_link.end_col,
                                );

                                self.copied_link_ids.insert(copied_link.id, link.id);
                                link_copies.push(copied_link);
                                copied_link.edited = true;
                                copied_link.id = self.next_link_id;
                                self.links.insert(copied_link.id, copied_link);
                                self.next_link_id += 1;
                            }
                        }
                    }
                    self.insert_piece(s, Some(link_copies))
                } else {
                    self.insert_piece(s, None)
                }
            }
            YankText::Chunk((c, l, _)) => {
                let mut link_copies = vec![];
                if let Some(yanked_links) = l {
                    for yanked_link in yanked_links.iter() {
                        let link = self
                            .links
                            .get_mut(&yanked_link.id)
                            .expect("Link to paste should be in links hashmap");

                        match link.deleted {
                            true => {
                                link.deleted = false;
                                link.edited = true;
                                link.row = row + yanked_link.row;
                                link.start_col = col + yanked_link.start_col;
                                link.end_col = col + yanked_link.end_col;
                            }
                            false => {
                                let (start_col_offset, end_col_offset) = if yanked_link.row == 0 {
                                    (col + yanked_link.start_col, col + yanked_link.end_col)
                                } else {
                                    (link.start_col, link.end_col)
                                };
                                let mut copied_link = Link::new(
                                    self.next_link_id,
                                    row + yanked_link.row,
                                    start_col_offset,
                                    end_col_offset,
                                );
                                self.copied_link_ids.insert(copied_link.id, link.id);
                                link_copies.push(copied_link);
                                copied_link.edited = true;
                                self.links.insert(copied_link.id, copied_link);
                                self.next_link_id += 1;
                            }
                        }
                    }
                    self.insert_chunk(c, Some(link_copies))
                } else {
                    self.insert_chunk(c, None)
                }
            }
        }
    }

    /// Start text selection at the cursor position. If text selection is already ongoing, the start position is reset.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["aaa bbb ccc"]);
    ///
    /// textarea.start_selection();
    /// textarea.move_cursor(CursorMove::WordForward);
    /// textarea.copy();
    /// assert_eq!(textarea.yank_text(), "aaa ");
    /// ```
    pub fn start_selection(&mut self) {
        self.selection_start = Some(self.cursor);
    }

    /// Stop the current text selection. This method does nothing if text selection is not ongoing.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["aaa bbb ccc"]);
    ///
    /// textarea.start_selection();
    /// textarea.move_cursor(CursorMove::WordForward);
    ///
    /// // Cancel the ongoing text selection
    /// textarea.cancel_selection();
    ///
    /// // As the result, this `copy` call does nothing
    /// textarea.copy();
    /// assert_eq!(textarea.yank_text(), "");
    /// ```
    pub fn cancel_selection(&mut self) {
        self.selection_start = None;
    }

    /// Select the entire text. Cursor moves to the end of the text buffer. When text selection is already ongoing,
    /// it is canceled.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["aaa", "bbb", "ccc"]);
    ///
    /// textarea.select_all();
    ///
    /// // Cut the entire text;
    /// textarea.cut();
    ///
    /// assert_eq!(textarea.lines(), [""]); // Buffer is now empty
    /// assert_eq!(textarea.yank_text(), "aaa\nbbb\nccc");
    /// ```
    pub fn select_all(&mut self) {
        self.move_cursor(CursorMove::Jump(u16::MAX, u16::MAX));
        self.selection_start = Some((0, 0));
    }

    /// Return if text selection is ongoing or not.
    /// ```
    /// use tuipaz_textarea::{TextArea};
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// assert!(!textarea.is_selecting());
    /// textarea.start_selection();
    /// assert!(textarea.is_selecting());
    /// textarea.cancel_selection();
    /// assert!(!textarea.is_selecting());
    /// ```
    pub fn is_selecting(&self) -> bool {
        self.selection_start.is_some()
    }

    fn line_offset(&self, row: usize, col: usize) -> usize {
        let line = self
            .lines
            .get(row)
            .unwrap_or(&self.lines[self.lines.len() - 1]);
        line.char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len())
    }

    /// Set the style used for text selection. The default style is light blue.
    /// ```
    /// use tuipaz_textarea::TextArea;
    /// use ratatui::style::{Style, Color};
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// // Change the selection color from the default to Red
    /// textarea.set_selection_style(Style::default().bg(Color::Red));
    /// assert_eq!(textarea.selection_style(), Style::default().bg(Color::Red));
    /// ```
    pub fn set_selection_style(&mut self, style: Style) {
        self.select_style = style;
    }

    /// Get the style used for text selection.
    /// ```
    /// use tuipaz_textarea::TextArea;
    /// use ratatui::style::{Style, Color};
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// assert_eq!(textarea.selection_style(), Style::default().bg(Color::LightBlue));
    /// ```
    pub fn selection_style(&mut self) -> Style {
        self.select_style
    }

    fn selection_range(&self) -> Option<(Pos, Pos)> {
        let (sr, sc) = self.selection_start?;
        let (er, ec) = self.cursor;
        let (so, eo) = (self.line_offset(sr, sc), self.line_offset(er, ec));
        let s = Pos::new(sr, sc, so);
        let e = Pos::new(er, ec, eo);
        match (sr, so).cmp(&(er, eo)) {
            Ordering::Less => Some((s, e)),
            Ordering::Equal => None,
            Ordering::Greater => Some((e, s)),
        }
    }

    pub fn get_selection_start(&self) -> Option<(usize, usize)> {
        self.selection_start
    }

    fn take_selection_range(&mut self) -> Option<(Pos, Pos)> {
        let range = self.selection_range();
        self.cancel_selection();
        range
    }

    /// Copy the selection text to the yank buffer. When nothing is selected, this method does nothing.
    /// To get the yanked text, use [`TextArea::yank_text`].
    /// ```
    /// use tuipaz_textarea::{TextArea, Key, Input, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["Hello World"]);
    ///
    /// // Start text selection at 'W'
    /// textarea.move_cursor(CursorMove::WordForward);
    /// textarea.start_selection();
    ///
    /// // Select the word "World" and copy the selected text
    /// textarea.move_cursor(CursorMove::End);
    /// textarea.copy();
    ///
    /// assert_eq!(textarea.yank_text(), "World");
    /// assert_eq!(textarea.lines(), ["Hello World"]); // Text does not change
    /// ```
    pub fn copy(&mut self) {
        if let Some((start, end)) = self.take_selection_range() {
            let l = self
                .links
                .iter()
                .filter(|(_, l)| Self::link_in_range(l, &start, &end))
                .map(|(id, l)| {
                    let row = l.row - start.row;
                    let (start_col, end_col) = if row == 0 {
                        (l.start_col - start.col, l.end_col - start.col)
                    } else {
                        (l.start_col, l.end_col)
                    };

                    Link::new(*id, row, start_col, end_col)
                })
                .collect::<Vec<Link>>();

            let links = match l.is_empty() {
                true => None,
                false => Some(l),
            };

            info!("textarea::copy::links: {:?}", links);

            if start.row == end.row {
                let text = self.lines[start.row][start.offset..end.offset].to_string();

                self.yank = YankText::Piece((text, links, (start.row, start.col)));
            } else {
                let mut chunk = vec![self.lines[start.row][start.offset..].to_string()];
                chunk.extend(self.lines[start.row + 1..end.row].iter().cloned());
                chunk.push(self.lines[end.row][..end.offset].to_string());
                self.yank = YankText::Chunk((chunk, links, (start.row, start.col)));
            }
        }
    }

    pub fn link_in_range(l: &&Link, start: &Pos, end: &Pos) -> bool {
        if start.row == end.row && l.row == start.row {
            l.start_col >= start.col && l.end_col <= end.col
        } else if l.row == start.row {
            l.start_col >= start.col
        } else if l.row == end.row {
            l.end_col <= end.col
        } else {
            l.row > start.row && l.row < end.row
        }
    }

    /// Cut the selected text and place it in the yank buffer. This method returns whether the text was modified.
    /// The cursor will move to the start position of the text selection.
    /// To get the yanked text, use [`TextArea::yank_text`].
    /// ```
    /// use tuipaz_textarea::{TextArea, Key, Input, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["Hello World"]);
    ///
    /// // Start text selection at 'W'
    /// textarea.move_cursor(CursorMove::WordForward);
    /// textarea.start_selection();
    ///
    /// // Select the word "World" and copy the selected text
    /// textarea.move_cursor(CursorMove::End);
    /// textarea.cut();
    ///
    /// assert_eq!(textarea.yank_text(), "World");
    /// assert_eq!(textarea.lines(), ["Hello "]);
    /// ```
    pub fn cut(&mut self) -> bool {
        self.delete_selection(true)
    }

    fn delete_selection(&mut self, should_yank: bool) -> bool {
        info!("INSIDE delete_selection");
        if let Some((s, e)) = self.take_selection_range() {
            info!("selection range => s: {:?}, e: {:?}", s, e);
            let deleted_links = self.delete_links_in_range((s.row, s.col), (e.row, e.col));
            self.shift_links_after_delete((e.row, e.col), (s.row, s.col), 0);

            self.delete_range(s, e, deleted_links, should_yank);
            return true;
        }
        false
    }

    /// Move the cursor to the position specified by the [`CursorMove`] parameter. For each kind of cursor moves, see
    /// the document of [`CursorMove`].
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["abc", "def"]);
    ///
    /// textarea.move_cursor(CursorMove::Forward);
    /// assert_eq!(textarea.cursor(), (0, 1));
    /// textarea.move_cursor(CursorMove::Down);
    /// assert_eq!(textarea.cursor(), (1, 1));
    /// ```
    pub fn move_cursor(&mut self, m: CursorMove) {
        self.move_cursor_with_shift(m, self.selection_start.is_some());
    }

    fn move_cursor_with_shift(&mut self, m: CursorMove, shift: bool) {
        if let Some(cursor) = m.next_cursor(self.cursor, &self.lines, &self.viewport) {
            if shift {
                if self.selection_start.is_none() {
                    self.start_selection();
                }
            } else {
                self.cancel_selection();
            }
            self.cursor = cursor;
        }
    }

    /// Undo the last modification. This method returns if the undo modified text contents or not in the textarea.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["abc def"]);
    ///
    /// textarea.delete_next_word();
    /// assert_eq!(textarea.lines(), [" def"]);
    /// textarea.undo();
    /// assert_eq!(textarea.lines(), ["abc def"]);
    /// ```
    pub fn undo(&mut self) -> bool {
        if let Some((cursor_before, cursor_after)) =
            self.history.undo(&mut self.lines, &mut self.links)
        {
            info!("fn undo::self.links: {:?}", self.links);
            self.cancel_selection();
            self.shift_links_after_edit(cursor_after, cursor_before);
            self.cursor = cursor_before;
            true
        } else {
            false
        }
    }

    /// Redo the last undo change. This method returns if the redo modified text contents or not in the textarea.
    /// ```
    /// use tuipaz_textarea::{TextArea, CursorMove};
    ///
    /// let mut textarea = TextArea::from(["abc def"]);
    ///
    /// textarea.delete_next_word();
    /// assert_eq!(textarea.lines(), [" def"]);
    /// textarea.undo();
    /// assert_eq!(textarea.lines(), ["abc def"]);
    /// textarea.redo();
    /// assert_eq!(textarea.lines(), [" def"]);
    /// ```
    pub fn redo(&mut self) -> bool {
        info!("inside textarea.rs redo");
        if let Some((cursor_before, cursor_after)) =
            self.history.redo(&mut self.lines, &mut self.links)
        {
            self.cancel_selection();
            self.shift_links_after_edit(cursor_before, cursor_after);
            self.cursor = cursor_after;
            true
        } else {
            false
        }
    }

    pub(crate) fn line_spans<'b>(&'b self, line: &'b str, row: usize, lnum_len: u8) -> Line<'b> {
        let mut hl = LineHighlighter::new(
            line,
            self.cursor_style,
            self.link_style,
            self.tab_len,
            self.mask,
            self.select_style,
        );

        if let Some(style) = self.line_number_style {
            hl.line_number(row, lnum_len, style);
        }

        if row == self.cursor.0 {
            hl.cursor_line(self.cursor.1, self.cursor_line_style);
        }

        if let Some(matches) = self.search.matches(line) {
            hl.search(matches, self.search.style);
        }

        let mut count = self.hop.get_count();
        if self.hopping {
            let mut match_vec = vec![];

            if let Some(matches) = self.hop.matches(line) {
                (match_vec, count) = hl.hop(matches, self.hop.style, count);
            }

            if self.hop_pending {
                self.hop.set_match_indexes(match_vec, row);
            }
        }
        self.hop.set_count(count);

        hl.links(&self.links, row, self.link_style);

        if let Some((start, end)) = self.selection_range() {
            hl.selection(row, start.row, start.offset, end.row, end.offset);
        }

        hl.into_spans()
    }

    /// insert_link inserts a link at the current cursor position in the `TextArea`.
    /// Links are identified by unique IDs and span across one or more characters,
    /// but can't currently span multiple lines
    ///
    /// # Behavior
    ///
    ///  - pending_link = None` -> save cursor pos and char in self.pending_link
    ///  - pending_link` = Some(position) -> calculates the range between the stored `pending_link` position
    ///   and the current cursor position, creates a new `Link` object with these positions,
    ///   adds it to the `links` vector, increments the `next_link_id`, and resets `pending_link` to `None`.
    ///
    pub fn init_link(&mut self) {
        self.pending_link = Some((self.cursor.0, self.cursor.1));
    }

    pub fn insert_link(&mut self) {
        if let Some(link_start) = self.pending_link {
            if link_start.0 == self.cursor.0 && link_start.1 <= self.cursor.1 {
                let new_link =
                    Link::new(self.next_link_id, link_start.0, link_start.1, self.cursor.1);
                self.links.insert(self.next_link_id, new_link);
                self.next_link_id += 1;
                self.pending_link = None;
                self.new_link = true;
            }
        }
    }

    pub fn delete_link(&mut self, link_id: usize) -> usize {
        info!("inside delete_link");
        self.deleted_link_ids.push(link_id);
        let link = self
            .links
            .get_mut(&link_id)
            .expect("link to delete should be present");
        link.toggle_deleted();
        link_id
    }

    /// in_link checks if the cursor's current position (`cpos`) falls within any of the defined links in the `TextArea`.
    /// It returns the ID of the link if the cursor is inside a link; otherwise, it returns `None`.
    ///
    /// # Arguments
    ///  - `cpos` -> A tuple containing the row and column indices of the cursor's current position.
    ///
    /// # Returns
    ///  - Some(id) -> ID of the link if the cursor is inside a links
    ///  - None -> if the cursor is not inside a link.
    ///
    pub fn in_link(&self, cpos: (usize, usize)) -> Option<usize> {
        for (id, link) in self.links.iter().filter(|(_, link)| !link.deleted) {
            // No links on cursor row
            if cpos.0 != link.row {
                continue;
            } else if cpos.1 >= link.start_col && cpos.1 <= link.end_col {
                return Some(*id);
            }
        }
        // No links
        None
    }

    pub fn links_in_row_before_cursor(&self, cpos: (usize, usize)) -> Option<Vec<usize>> {
        let mut id_vec = vec![];
        info!("links_in_row_before_cursor::cpos: {:?}", cpos);
        for (id, link) in self.links.iter().filter(|(_, link)| !link.deleted) {
            info!("links_in_row_before_cursor::link: {:?}", link);
            if cpos.0 != link.row {
                continue;
            } else if cpos.1 >= link.end_col {
                id_vec.push(*id);
            }
        }

        info!("links_in_row_before_cursor::id_vec: {:?}", id_vec);
        match id_vec.is_empty() {
            true => None,
            false => Some(id_vec),
        }
    }

    pub fn links_in_row_after_cursor(&self, cpos: (usize, usize)) -> Option<Vec<usize>> {
        let mut id_vec = vec![];
        info!("links_in_row_after_cursor::cpos: {:?}", cpos);
        for (id, link) in self.links.iter().filter(|(_, link)| !link.deleted) {
            info!("links_in_row_after_cursor::link: {:?}", link);
            if cpos.0 != link.row {
                continue;
            } else if cpos.1 < link.start_col {
                id_vec.push(*id);
            }
        }

        info!("links_in_row_after_cursor::id_vec: {:?}", id_vec);
        match id_vec.is_empty() {
            true => None,
            false => Some(id_vec),
        }
    }

    pub fn shift_links_same_row(&mut self, row: usize, (start_col, end_col): (usize, usize)) {
        info!("shift_links_same_row");
        let dcol = end_col as i64 - start_col as i64;
        for l in self
            .links
            .values_mut()
            .filter(|l| l.row == row && l.start_col >= start_col && !l.deleted)
        {
            l.start_col = match (l.start_col as i64 + dcol) as usize {
                std::usize::MAX => 0,
                n => n,
            };
            l.end_col = match (l.end_col as i64 + dcol) as usize {
                std::usize::MAX => 0,
                n => n,
            };
        }
    }

    pub fn shift_links_after_delete(
        &mut self,
        (start_row, start_col): (usize, usize),
        (end_row, end_col): (usize, usize),
        shifted_to_prevline: usize,
    ) {
        let drow = end_row as i64 - start_row as i64;
        let dcol = end_col as i64 - start_col as i64;

        info!(
            "shift_links_after_delete::{}",
            log_format(&(start_row, end_row), "(start_row, end_row)")
        );
        info!(
            "shift_links_after_delete::{}",
            log_format(&(start_col, end_col), "(start_col, end_col)")
        );
        info!(
            "shift_links_after_delete::{}",
            log_format(&(drow, dcol), "(drow, dcol)")
        );
        for l in self.links.values_mut().filter(|l| !l.deleted) {
            info!("link before shift{}", log_format(&l, ""));
            let prev_start_col = l.start_col;
            let prev_end_col = l.end_col;

            if l.row >= start_row {
                if l.row == start_row && l.start_col >= start_col {
                    info!("shifting columns");
                    (l.start_col, l.end_col) = match dcol >= 0 {
                        true => (
                            l.start_col.saturating_add(dcol as usize),
                            l.end_col.saturating_add(dcol as usize),
                        ),
                        false => {
                            let positive_dcol = dcol.unsigned_abs() as usize;
                            (
                                l.start_col.saturating_sub(positive_dcol),
                                l.end_col.saturating_sub(positive_dcol),
                            )
                        }
                    }
                }
                info!("l.start_col 1: {}", l.start_col);
                info!("l.end_col 1: {}", l.end_col);
                let max_col = self.max_col as usize;

                if l.end_col < max_col {
                    (l.row) = match drow >= 0 {
                        true => l.row.saturating_add(drow as usize),
                        false => {
                            let positive_drow = drow.unsigned_abs() as usize;
                            l.row.saturating_sub(positive_drow)
                        }
                    }
                } else {
                    l.end_col = prev_end_col - shifted_to_prevline;
                    l.start_col = prev_start_col - shifted_to_prevline;

                    info!("l.start_col 2: {}", l.start_col);
                    info!("l.end_col 2: {}", l.end_col);
                }
            }
            info!("shift_links_after_delete::new pos: {:?}", l);
        }
    }

    pub fn shift_links_after_insert(
        &mut self,
        (start_row, start_col): (usize, usize),
        (end_row, end_col): (usize, usize),
    ) {
        let drow = end_row as i64 - start_row as i64;
        let dcol = end_col as i64 - start_col as i64;

        info!(
            "SHIFT LINKS AFTER INSERT
            \ncursor: {:?}
            \nmax_col: {}
            \n(start_row, end_row): {:?}
            \n(start_col, end_col): {:?}
            \n(drow, dcol): {:?}
            ",
            self.cursor,
            self.max_col,
            (start_row, end_row),
            (start_col, end_col),
            (drow, dcol),
        );

        for l in self.links.values_mut().filter(|l| !l.deleted) {
            info!("shift_links_after_insert::link BEFORE: {:?}", l);
            if l.edited {
                info!("link edited: {:?}", l);
                l.edited = false;
            } else if l.row == start_row && l.start_col >= start_col {
                (l.start_col, l.end_col) = match dcol >= 0 {
                    true => (
                        l.start_col.saturating_add(dcol as usize),
                        l.end_col.saturating_add(dcol as usize),
                    ),
                    false => {
                        let positive_dcol = dcol.unsigned_abs() as usize;
                        (
                            l.start_col.saturating_sub(positive_dcol),
                            l.end_col.saturating_sub(positive_dcol),
                        )
                    }
                };

                l.row = match drow >= 0 {
                    true => l.row.saturating_add(drow as usize),
                    false => {
                        let positive_drow = drow.unsigned_abs() as usize;
                        l.row.saturating_sub(positive_drow)
                    }
                };
            } else if l.row > start_row {
                l.row = match drow >= 0 {
                    true => l.row.saturating_add(drow as usize),
                    false => {
                        let positive_drow = drow.unsigned_abs() as usize;
                        l.row.saturating_sub(positive_drow)
                    }
                };
            }
            info!("shift_links_after_insert::link AFTER: {:?}", l);
        }
    }

    pub fn shift_links_after_edit(
        &mut self,
        (start_row, start_col): (usize, usize),
        (end_row, end_col): (usize, usize),
    ) {
        info!("SHIFT LINKS AFTER EDIT");
        let drow = end_row as i64 - start_row as i64;
        let dcol = end_col as i64 - start_col as i64;
        // Dont shift links that were in the edit or are currently deleted
        for l in self.links.values_mut().filter(|l| !l.deleted) {
            info!("shift_links_after_edit::link BEFORE: {:?}", l);
            if l.row >= start_row && !l.edited {
                if (l.row == end_row && l.start_col >= start_col)
                    || (l.row == start_row && end_row != start_row)
                {
                    (l.start_col, l.end_col) = match dcol >= 0 {
                        true => (
                            l.start_col.saturating_add(dcol as usize),
                            l.end_col.saturating_add(dcol as usize),
                        ),
                        false => {
                            let positive_dcol = dcol.unsigned_abs() as usize;
                            (
                                l.start_col.saturating_sub(positive_dcol),
                                l.end_col.saturating_sub(positive_dcol),
                            )
                        }
                    }
                }

                (l.row) = match drow >= 0 {
                    true => l.row.saturating_add(drow as usize),
                    false => {
                        let positive_drow = drow.unsigned_abs() as usize;
                        l.row.saturating_sub(positive_drow)
                    }
                };
            } else {
                l.toggle_edited();
            }
            info!("shift_links_after_edit::link AFTER: {:?}", l);
        }
    }

    pub fn shift_links_prevline(&mut self, (row, col): (usize, usize), dcol: usize) {
        info!("SHIFT LINKS PREVLINE");
        for l in self.links.values_mut() {
            info!("{}", log_format(&l, "l before shift"));
            info!("{}", log_format(&dcol, "dcol"));
            info!("{}", log_format(&(row, col), "(row, col)"));
            if l.row >= row {
                l.row = l.row.saturating_sub(1);
                l.start_col = l.start_col.saturating_add(dcol);
                l.end_col = l.end_col.saturating_add(dcol);
            }
            info!("{}", log_format(&l, "l after shift"));
        }
    }

    pub fn shift_links_newline(&mut self, (row, col): (usize, usize)) {
        for l in self.links.values_mut() {
            info!("shift_links_newline::link before shift: {:?}", l);
            if l.edited {
                info!("shift_links_newline::ignoring edited link");
                break;
            } else if l.row > row {
                l.row = l.row.saturating_add(1);
            } else if l.row == row && l.start_col >= col {
                l.row = l.row.saturating_add(1);
                l.start_col = l.start_col.saturating_sub(col);
                l.end_col = l.end_col.saturating_sub(col);
            }
            info!("{}", log_format(&l, "l after shift"));
        }
    }

    pub fn delete_links_in_range(
        &mut self,
        start: (usize, usize),
        end: (usize, usize),
    ) -> MaybeLinks {
        let mut deleted_links = Vec::new();
        for (id, link) in self.links.iter() {
            if (link.row < start.0 || link.row > end.0)
                || (link.row == start.0 && link.end_col < start.1)
                || (link.row == end.0 && link.start_col > end.1)
            {
                continue;
            } else {
                let row_offset = link.row - start.0;
                let (start_col_offset, end_col_offset) = if row_offset == 0 {
                    (link.start_col - start.1, link.end_col - start.1)
                } else {
                    (link.start_col, link.end_col)
                };

                let deleted_link = Link::new(*id, row_offset, start_col_offset, end_col_offset);
                deleted_links.push(deleted_link);
            }
        }

        if deleted_links.is_empty() {
            None
        } else {
            for dl in deleted_links.iter() {
                self.delete_link(dl.id);
            }
            Some(deleted_links)
        }
    }

    ///cBuild a ratatui (or tui-rs) widget to render the current state of the textarea. The widget instance returned
    /// from this method can be rendered with [`ratatui::terminal::Frame::render_widget`].
    /// ```no_run
    /// use ratatui::backend::CrosstermBackend;
    /// use ratatui::layout::{Constraint, Direction, Layout};
    /// use ratatui::Terminal;
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// let layout = Layout::default()
    ///     .direction(Direction::Vertical)
    ///     .constraints([Constraint::Min(1)].as_ref());
    /// let backend = CrosstermBackend::new(std::io::stdout());
    /// let mut term = Terminal::new(backend).unwrap();
    ///
    /// loop {
    ///     term.draw(|f| {
    ///         let chunks = layout.split(f.size());
    ///         let widget = textarea.widget();
    ///         f.render_widget(widget, chunks[0]);
    ///     }).unwrap();
    ///
    ///     // ...
    /// }
    /// ```
    pub fn widget(&'a self) -> impl Widget + 'a {
        Renderer::new(self)
    }

    /// Set the style of textarea. By default, textarea is not styled.
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    /// let style = Style::default().fg(Color::Red);
    /// textarea.set_style(style);
    /// assert_eq!(textarea.style(), style);
    /// ```
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    /// Get the current style of textarea.
    pub fn style(&self) -> Style {
        self.style
    }

    /// Set the block of textarea. By default, no block is set.
    /// ```
    /// use tuipaz_textarea::TextArea;
    /// use ratatui::widgets::{Block, Borders};
    ///
    /// let mut textarea = TextArea::default();
    /// let block = Block::default().borders(Borders::ALL).title("Block Title");
    /// textarea.set_block(block);
    /// assert!(textarea.block().is_some());
    /// ```
    pub fn set_block(&mut self, block: Block<'a>) {
        self.block = Some(block);
    }

    /// Remove the block of textarea which was set by [`TextArea::set_block`].
    /// ```
    /// use tuipaz_textarea::TextArea;
    /// use ratatui::widgets::{Block, Borders};
    ///
    /// let mut textarea = TextArea::default();
    /// let block = Block::default().borders(Borders::ALL).title("Block Title");
    /// textarea.set_block(block);
    /// textarea.remove_block();
    /// assert!(textarea.block().is_none());
    /// ```
    pub fn remove_block(&mut self) {
        self.block = None;
    }

    /// Get the block of textarea if exists.
    pub fn block<'s>(&'s self) -> Option<&'s Block<'a>> {
        self.block.as_ref()
    }

    /// Set the length of tab character. Setting 0 disables tab inputs.
    /// ```
    /// use tuipaz_textarea::{TextArea, Input, Key};
    ///
    /// let mut textarea = TextArea::default();
    /// let tab_input = Input { key: Key::Tab, ctrl: false, alt: false, shift: false };
    ///
    /// textarea.set_tab_length(8);
    /// textarea.input(tab_input.clone());
    /// assert_eq!(textarea.lines(), ["        "]);
    ///
    /// textarea.set_tab_length(2);
    /// textarea.input(tab_input);
    /// assert_eq!(textarea.lines(), ["          "]);
    /// ```
    pub fn set_tab_length(&mut self, len: u8) {
        self.tab_len = len;
    }

    /// Get how many spaces are used for representing tab character. The default value is 4.
    pub fn tab_length(&self) -> u8 {
        self.tab_len
    }

    /// Set if a hard tab is used or not for indent. When `true` is set, typing a tab key inserts a hard tab instead of
    /// spaces. By default, hard tab is disabled.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.set_hard_tab_indent(true);
    /// textarea.insert_tab();
    /// assert_eq!(textarea.lines(), ["\t"]);
    /// ```
    pub fn set_hard_tab_indent(&mut self, enabled: bool) {
        self.hard_tab_indent = enabled;
    }

    /// Get if a hard tab is used for indent or not.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// assert!(!textarea.hard_tab_indent());
    /// textarea.set_hard_tab_indent(true);
    /// assert!(textarea.hard_tab_indent());
    /// ```
    pub fn hard_tab_indent(&self) -> bool {
        self.hard_tab_indent
    }

    /// Get a string for indent. It consists of spaces by default. When hard tab is enabled, it is a tab character.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// assert_eq!(textarea.indent(), "    ");
    /// textarea.set_tab_length(2);
    /// assert_eq!(textarea.indent(), "  ");
    /// textarea.set_hard_tab_indent(true);
    /// assert_eq!(textarea.indent(), "\t");
    /// ```
    pub fn indent(&self) -> &'static str {
        if self.hard_tab_indent {
            "\t"
        } else {
            spaces(self.tab_len)
        }
    }

    /// Set how many modifications are remembered for undo/redo. Setting 0 disables undo/redo.
    pub fn set_max_histories(&mut self, max: usize) {
        self.history = History::new(max);
    }

    /// Get how many modifications are remembered for undo/redo. The default value is 50.
    pub fn max_histories(&self) -> usize {
        self.history.max_items()
    }

    /// Set the style of line at cursor. By default, the cursor line is styled with underline. To stop styling the
    /// cursor line, set the default style.
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// let style = Style::default().fg(Color::Red);
    /// textarea.set_cursor_line_style(style);
    /// assert_eq!(textarea.cursor_line_style(), style);
    ///
    /// // Disable cursor line style
    /// textarea.set_cursor_line_style(Style::default());
    /// ```
    pub fn set_cursor_line_style(&mut self, style: Style) {
        self.cursor_line_style = style;
    }

    /// Get the style of cursor line. By default it is styled with underline.
    pub fn cursor_line_style(&self) -> Style {
        self.cursor_line_style
    }

    /// Set the style of line number. By setting the style with this method, line numbers are drawn in textarea, meant
    /// that line numbers are disabled by default. If you want to show line numbers but don't want to style them, set
    /// the default style.
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// // Show line numbers in dark gray background
    /// let style = Style::default().bg(Color::DarkGray);
    /// textarea.set_line_number_style(style);
    /// assert_eq!(textarea.line_number_style(), Some(style));
    /// ```
    pub fn set_line_number_style(&mut self, style: Style) {
        self.line_number_style = Some(style);
    }

    /// Remove the style of line number which was set by [`TextArea::set_line_number_style`]. After calling this
    /// method, Line numbers will no longer be shown.
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.set_line_number_style(Style::default().bg(Color::DarkGray));
    /// textarea.remove_line_number();
    /// assert_eq!(textarea.line_number_style(), None);
    /// ```
    pub fn remove_line_number(&mut self) {
        self.line_number_style = None;
    }

    /// Get the style of line number if set.
    pub fn line_number_style(&self) -> Option<Style> {
        self.line_number_style
    }

    /// Set the placeholder text. The text is set in the textarea when no text is input. Setting a non-empty string `""`
    /// enables the placeholder. The default value is an empty string so the placeholder is disabled by default.
    /// To customize the text style, see [`TextArea::set_placeholder_style`].
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    /// assert_eq!(textarea.placeholder_text(), "");
    /// assert!(textarea.placeholder_style().is_none());
    ///
    /// textarea.set_placeholder_text("Hello");
    /// assert_eq!(textarea.placeholder_text(), "Hello");
    /// assert!(textarea.placeholder_style().is_some());
    /// ```
    pub fn set_placeholder_text(&mut self, placeholder: impl Into<String>) {
        self.placeholder = placeholder.into();
    }

    /// Set the style of the placeholder text. The default style is a dark gray text.
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    /// assert_eq!(textarea.placeholder_style(), None); // When the placeholder is disabled
    ///
    /// textarea.set_placeholder_text("Enter your message"); // Enable placeholder by setting non-empty text
    ///
    /// let style = Style::default().bg(Color::Blue);
    /// textarea.set_placeholder_style(style);
    /// assert_eq!(textarea.placeholder_style(), Some(style));
    /// ```
    pub fn set_placeholder_style(&mut self, style: Style) {
        self.placeholder_style = style;
    }

    /// Get the placeholder text. An empty string means the placeholder is disabled. The default value is an empty string.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let textarea = TextArea::default();
    /// assert_eq!(textarea.placeholder_text(), "");
    /// ```
    pub fn placeholder_text(&self) -> &'_ str {
        self.placeholder.as_str()
    }

    /// Get the placeholder style. When the placeholder text is empty, it returns `None` since the placeholder is disabled.
    /// The default style is a dark gray text.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    /// assert_eq!(textarea.placeholder_style(), None);
    ///
    /// textarea.set_placeholder_text("hello");
    /// assert!(textarea.placeholder_style().is_some());
    /// ```
    pub fn placeholder_style(&self) -> Option<Style> {
        if self.placeholder.is_empty() {
            None
        } else {
            Some(self.placeholder_style)
        }
    }

    /// Specify a character masking the text. All characters in the textarea will be replaced by this character.
    /// This API is useful for making a kind of credentials form such as a password input.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.set_mask_char('*');
    /// assert_eq!(textarea.mask_char(), Some('*'));
    /// textarea.set_mask_char('●');
    /// assert_eq!(textarea.mask_char(), Some('●'));
    /// ```
    pub fn set_mask_char(&mut self, mask: char) {
        self.mask = Some(mask);
    }

    /// Clear the masking character previously set by [`TextArea::set_mask_char`].
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.set_mask_char('*');
    /// assert_eq!(textarea.mask_char(), Some('*'));
    /// textarea.clear_mask_char();
    /// assert_eq!(textarea.mask_char(), None);
    /// ```
    pub fn clear_mask_char(&mut self) {
        self.mask = None;
    }

    /// Get the character to mask text. When no character is set, `None` is returned.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// assert_eq!(textarea.mask_char(), None);
    /// textarea.set_mask_char('*');
    /// assert_eq!(textarea.mask_char(), Some('*'));
    /// ```
    pub fn mask_char(&self) -> Option<char> {
        self.mask
    }

    /// Set the style of cursor. By default, a cursor is rendered in the reversed color. Setting the same style as
    /// cursor line hides a cursor.
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// let style = Style::default().bg(Color::Red);
    /// textarea.set_cursor_style(style);
    /// assert_eq!(textarea.cursor_style(), style);
    /// ```
    pub fn set_cursor_style(&mut self, style: Style) {
        self.cursor_style = style;
    }

    /// Get the style of cursor.
    pub fn cursor_style(&self) -> Style {
        self.cursor_style
    }

    /// Get slice of line texts. This method borrows the content, but not moves. Note that the returned slice will
    /// never be empty because an empty text means a slice containing one empty line. This is correct since any text
    /// file must end with a newline.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    /// assert_eq!(textarea.lines(), [""]);
    ///
    /// textarea.insert_char('a');
    /// assert_eq!(textarea.lines(), ["a"]);
    ///
    /// textarea.insert_newline();
    /// assert_eq!(textarea.lines(), ["a", ""]);
    ///
    /// textarea.insert_char('b');
    /// assert_eq!(textarea.lines(), ["a", "b"]);
    /// ```
    pub fn lines(&'a self) -> &'a [String] {
        &self.lines
    }

    /// Convert [`TextArea`] instance into line texts.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.insert_char('a');
    /// textarea.insert_newline();
    /// textarea.insert_char('b');
    ///
    /// assert_eq!(textarea.into_lines(), ["a", "b"]);
    /// ```
    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }

    /// Get the current cursor position. 0-base character-wise (row, col) cursor position.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    /// assert_eq!(textarea.cursor(), (0, 0));
    ///
    /// textarea.insert_char('a');
    /// textarea.insert_newline();
    /// textarea.insert_char('b');
    ///
    /// assert_eq!(textarea.cursor(), (1, 1));
    /// ```
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// Set text alignment. When [`Alignment::Center`] or [`Alignment::Right`] is set, line number is automatically
    /// disabled because those alignments don't work well with line numbers.
    /// ```
    /// use ratatui::layout::Alignment;
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.set_alignment(Alignment::Center);
    /// assert_eq!(textarea.alignment(), Alignment::Center);
    /// ```
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if let Alignment::Center | Alignment::Right = alignment {
            self.line_number_style = None;
        }
        self.alignment = alignment;
    }

    /// Get current text alignment. The default alignment is [`Alignment::Left`].
    /// ```
    /// use ratatui::layout::Alignment;
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// assert_eq!(textarea.alignment(), Alignment::Left);
    /// ```
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Check if the textarea has a empty content.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let textarea = TextArea::default();
    /// assert!(textarea.is_empty());
    ///
    /// let textarea = TextArea::from(["hello"]);
    /// assert!(!textarea.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.lines == [""]
    }

    /// Get the yanked text. Text is automatically yanked when deleting strings by [`TextArea::delete_line_by_head`],
    /// [`TextArea::delete_line_by_end`], [`TextArea::delete_word`], [`TextArea::delete_next_word`],
    /// [`TextArea::delete_str`], [`TextArea::copy`], and [`TextArea::cut`]. When multiple lines were yanked, they are
    /// always joined with `\n`.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::from(["abc"]);
    /// textarea.delete_next_word();
    /// assert_eq!(textarea.yank_text(), "abc");
    ///
    /// // Multiple lines are joined with \n
    /// let mut textarea = TextArea::from(["abc", "def"]);
    /// textarea.delete_str(5);
    /// assert_eq!(textarea.yank_text(), "abc\nd");
    /// ```
    pub fn yank_text(&self) -> String {
        self.yank.to_string()
    }

    /// Set a yanked text. The text can be inserted by [`TextArea::paste`]. `\n` and `\r\n` are recognized as newline
    /// but `\r` isn't.
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// textarea.set_yank_text("hello\nworld");
    /// textarea.paste();
    /// assert_eq!(textarea.lines(), ["hello", "world"]);
    /// ```
    pub fn set_yank_text(&mut self, text: impl Into<String>) {
        // `str::lines` is not available since it strips a newline at end
        let lines: Vec<_> = text
            .into()
            .split('\n')
            .map(|s| s.strip_suffix('\r').unwrap_or(s).to_string())
            .collect();
        self.yank = YankText::Chunk((lines, None, (0, 0)));
    }

    /// Set a regular expression pattern for text search. Setting an empty string stops the text search.
    /// When a valid pattern is set, all matches will be highlighted in the textarea. Note that the cursor does not
    /// move. To move the cursor, use [`TextArea::search_forward`] and [`TextArea::search_back`].
    ///
    /// Grammar of regular expression follows [regex crate](https://docs.rs/regex/latest/regex). Patterns don't match
    /// to newlines so match passes across no newline.
    ///
    /// When the pattern is invalid, the search pattern will not be updated and an error will be returned.
    ///
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::from(["hello, world", "goodbye, world"]);
    ///
    /// // Search "world"
    /// textarea.set_search_pattern("world").unwrap();
    ///
    /// assert_eq!(textarea.cursor(), (0, 0));
    /// textarea.search_forward(false);
    /// assert_eq!(textarea.cursor(), (0, 7));
    /// textarea.search_forward(false);
    /// assert_eq!(textarea.cursor(), (1, 9));
    ///
    /// // Stop the text search
    /// textarea.set_search_pattern("");
    ///
    /// // Invalid search pattern
    /// assert!(textarea.set_search_pattern("(hello").is_err());
    /// ```

    pub fn set_search_pattern(&mut self, query: impl AsRef<str>) -> Result<(), regex::Error> {
        self.search.set_pattern(query.as_ref())
    }

    /// Get a regular expression which was set by [`TextArea::set_search_pattern`]. When no text search is ongoing, this
    /// method returns `None`.
    ///
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// assert!(textarea.search_pattern().is_none());
    /// textarea.set_search_pattern("hello+").unwrap();
    /// assert!(textarea.search_pattern().is_some());
    /// assert_eq!(textarea.search_pattern().unwrap().as_str(), "hello+");
    /// ```

    pub fn search_pattern(&self) -> Option<&regex::Regex> {
        self.search.pat.as_ref()
    }

    pub fn clear_search(&mut self) {
        self.search.clear_pattern();
    }

    pub fn clear_lines(&mut self) {
        self.lines = vec!["".to_owned()];
    }

    /// Search the pattern set by [`TextArea::set_search_pattern`] forward and move the cursor to the next match
    /// position based on the current cursor position. Text search wraps around a text buffer. It returns `true` when
    /// some match was found. Otherwise it returns `false`.
    ///
    /// The `match_cursor` parameter represents if the search matches to the current cursor position or not. When `true`
    /// is set and the cursor position matches to the pattern, the cursor will not move. When `false`, the cursor will
    /// move to the next match ignoring the match at the current position.
    ///
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::from(["hello", "helloo", "hellooo"]);
    ///
    /// textarea.set_search_pattern("hello+").unwrap();
    ///
    /// // Move to next position
    /// let match_found = textarea.search_forward(false);
    /// assert!(match_found);
    /// assert_eq!(textarea.cursor(), (1, 0));
    ///
    /// // Since the cursor position matches to "hello+", it does not move
    /// textarea.search_forward(true);
    /// assert_eq!(textarea.cursor(), (1, 0));
    ///
    /// // When `match_current` parameter is set to `false`, match at the cursor position is ignored
    /// textarea.search_forward(false);
    /// assert_eq!(textarea.cursor(), (2, 0));
    ///
    /// // Text search wrap around the buffer
    /// textarea.search_forward(false);
    /// assert_eq!(textarea.cursor(), (0, 0));
    ///
    /// // `false` is returned when no match was found
    /// textarea.set_search_pattern("bye+").unwrap();
    /// let match_found = textarea.search_forward(false);
    /// assert!(!match_found);
    /// ```

    pub fn search_forward(&mut self, match_cursor: bool) -> bool {
        if let Some(cursor) = self.search.forward(&self.lines, self.cursor, match_cursor) {
            self.cursor = cursor;
            true
        } else {
            false
        }
    }

    /// Search the pattern set by [`TextArea::set_search_pattern`] backward and move the cursor to the next match
    /// position based on the current cursor position. Text search wraps around a text buffer. It returns `true` when
    /// some match was found. Otherwise it returns `false`.
    ///
    /// The `match_cursor` parameter represents if the search matches to the current cursor position or not. When `true`
    /// is set and the cursor position matches to the pattern, the cursor will not move. When `false`, the cursor will
    /// move to the next match ignoring the match at the current position.
    ///
    /// ```
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::from(["hello", "helloo", "hellooo"]);
    ///
    /// textarea.set_search_pattern("hello+").unwrap();
    ///
    /// // Move to next position with wrapping around the text buffer
    /// let match_found = textarea.search_back(false);
    /// assert!(match_found);
    /// assert_eq!(textarea.cursor(), (2, 0));
    ///
    /// // Since the cursor position matches to "hello+", it does not move
    /// textarea.search_back(true);
    /// assert_eq!(textarea.cursor(), (2, 0));
    ///
    /// // When `match_current` parameter is set to `false`, match at the cursor position is ignored
    /// textarea.search_back(false);
    /// assert_eq!(textarea.cursor(), (1, 0));
    ///
    /// // `false` is returned when no match was found
    /// textarea.set_search_pattern("bye+").unwrap();
    /// let match_found = textarea.search_back(false);
    /// assert!(!match_found);
    /// ```

    pub fn search_back(&mut self, match_cursor: bool) -> bool {
        if let Some(cursor) = self.search.back(&self.lines, self.cursor, match_cursor) {
            self.cursor = cursor;
            true
        } else {
            false
        }
    }

    /// Get the text style at matches of text search. The default style is colored with blue in background.
    ///
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let textarea = TextArea::default();
    ///
    /// assert_eq!(textarea.search_style(), Style::default().bg(Color::Blue));
    /// ```

    pub fn search_style(&self) -> Style {
        self.search.style
    }

    /// Set the text style at matches of text search. The default style is colored with blue in background.
    ///
    /// ```
    /// use ratatui::style::{Style, Color};
    /// use tuipaz_textarea::TextArea;
    ///
    /// let mut textarea = TextArea::default();
    ///
    /// let red_bg = Style::default().bg(Color::Red);
    /// textarea.set_search_style(red_bg);
    ///
    /// assert_eq!(textarea.search_style(), red_bg);
    /// ```

    pub fn set_search_style(&mut self, style: Style) {
        self.search.style = style;
    }

    pub fn hop_pattern(&self) -> Option<&regex::Regex> {
        self.hop.pat.as_ref()
    }

    pub fn clear_hop(&mut self) {
        self.hopping = false;
        self.hop_pending = false;
        self.hop.clear_pattern();
        self.hop.clear_match_indexes();
        self.hop.reset_count();
    }

    pub fn init_hop(&mut self) {
        self.hopping = true;
        self.hop_pending = true;
    }

    pub fn set_hop_pattern(&mut self, query: impl AsRef<str>) -> Result<(), regex::Error> {
        self.hop.set_pattern(query.as_ref())
    }

    pub fn hop_style(&self) -> Style {
        self.search.style
    }

    pub fn hop_to_idx(&mut self, idx: usize) {
        info!("hop_to_idx::self.hop: {:?}", self.hop);
        if let Some(match_idx) = self
            .hop
            .match_indexes
            .borrow()
            .iter()
            .find(|mi| mi.idx == idx)
        {
            info!("hop_to_idx: found match! {:?}", match_idx);
            self.cursor = match_idx.pos;
        }
    }

    /// Scroll the textarea. See [`Scrolling`] for the argument.
    /// The cursor will not move until it goes out the viewport. When the cursor position is outside the viewport after scroll,
    /// the cursor position will be adjusted to stay in the viewport using the same logic as [`CursorMove::InViewport`].
    ///
    /// ```
    /// # use ratatui::buffer::Buffer;
    /// # use ratatui::layout::Rect;
    /// # use ratatui::widgets::Widget;
    /// use tuipaz_textarea::TextArea;
    ///
    /// // Let's say terminal height is 8.
    ///
    /// // Create textarea with 20 lines "0", "1", "2", "3", ...
    /// let mut textarea: TextArea = (0..20).into_iter().map(|i| i.to_string()).collect();
    /// # // Call `render` at least once to populate terminal size
    /// # let r = Rect { x: 0, y: 0, width: 24, height: 8 };
    /// # let mut b = Buffer::empty(r.clone());
    /// # textarea.widget().render(r, &mut b);
    ///
    /// // Scroll down by 15 lines. Since terminal height is 8, cursor will go out
    /// // the viewport.
    /// textarea.scroll((15, 0));
    /// // So the cursor position was updated to stay in the viewport after the scrolling.
    /// assert_eq!(textarea.cursor(), (15, 0));
    ///
    /// // Scroll up by 5 lines. Since the scroll amount is smaller than the terminal
    /// // height, cursor position will not be updated.
    /// textarea.scroll((-5, 0));
    /// assert_eq!(textarea.cursor(), (15, 0));
    ///
    /// // Scroll up by 5 lines again. The terminal height is 8. So a cursor reaches to
    /// // the top of viewport after scrolling up by 7 lines. Since we have already
    /// // scrolled up by 5 lines, scrolling up by 5 lines again makes the cursor overrun
    /// // the viewport by 5 - 2 = 3 lines. To keep the cursor stay in the viewport, the
    /// // cursor position will be adjusted from line 15 to line 12.
    /// textarea.scroll((-5, 0));
    /// assert_eq!(textarea.cursor(), (12, 0));
    /// ```
    pub fn scroll(&mut self, scrolling: impl Into<Scrolling>) {
        self.scroll_with_shift(scrolling.into(), self.selection_start.is_some());
    }

    fn scroll_with_shift(&mut self, scrolling: Scrolling, shift: bool) {
        if shift && self.selection_start.is_none() {
            self.selection_start = Some(self.cursor);
        }
        scrolling.scroll(&mut self.viewport);
        self.move_cursor_with_shift(CursorMove::InViewport, shift);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME_COLOR: Color = Color::Red;

    fn ta_theme() -> TextAreaTheme {
        TextAreaTheme {
            text: THEME_COLOR,
            select: THEME_COLOR,
            links: THEME_COLOR,
            main_heading: THEME_COLOR,
            main_heading_modifiers: vec![Modifier::BOLD],
            sub_heading: THEME_COLOR,
            sub_heading_modifiers: vec![Modifier::BOLD],
        }
    }

    #[test]
    fn test_delete_piece_includes_link() {
        let mut textarea = TextArea::new(
            vec!["Hello [world]!".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 6,
                end_col: 11,
                edited: false,
                deleted: false,
            },
        );

        textarea.delete_piece(5, 10);

        assert_eq!(textarea.links.get(&0), None);
    }

    #[test]
    fn test_delete_entire_link() {
        let mut textarea = TextArea::new(vec!["[Link]".into()], HashMap::new(), 140, ta_theme());
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false,
            },
        );

        textarea.delete_piece(0, 5);

        assert_eq!(textarea.links.get(&0), None);
    }

    #[test]
    fn test_delete_part_of_link() {
        let mut textarea = TextArea::new(
            vec!["[Example Link]".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 21,
                edited: false,
                deleted: false,
            },
        );

        textarea.delete_piece(7, 9);

        assert_eq!(textarea.links.get(&0), None);
    }

    #[test]
    fn test_links_shift_on_insert_char() {
        let mut textarea = TextArea::new(
            vec!["Hello [world]!".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 6,
                end_col: 11,
                edited: false,
                deleted: false,
            },
        );
        textarea.move_cursor(CursorMove::Jump(0, 5));
        textarea.insert_char(' ');

        assert_eq!(textarea.links.get(&0).unwrap().start_col, 7);
        assert_eq!(textarea.links.get(&0).unwrap().end_col, 12);
    }

    #[test]
    fn test_links_shift_on_delete_char() {
        let mut textarea = TextArea::new(
            vec!["Hello [world]!".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 6,
                end_col: 11,
                edited: false,
                deleted: false,
            },
        );
        textarea.move_cursor(CursorMove::Jump(0, 5));
        textarea.delete_char();

        assert_eq!(textarea.links.get(&0).unwrap().start_col, 5);
        assert_eq!(textarea.links.get(&0).unwrap().end_col, 10);
    }

    #[test]
    fn test_deletion_on_different_rows_does_not_affect_links() {
        let mut textarea = TextArea::new(
            vec!["Line without link.".into(), "[Link]".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false,
            },
        );

        textarea.delete_piece(0, 5);

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_deletion_range_does_not_delete_links_but_shifts_it() {
        let mut textarea = TextArea::new(
            vec!["Before [link] and after.".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.delete_piece(0, 6);
        textarea.delete_piece(23, 13);

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 6,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_inserting_newline_above_and_below_link() {
        let mut textarea = TextArea::new(
            vec!["[Link]".into(), "Some text below.".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false,
            },
        );
        textarea.move_cursor(CursorMove::Jump(1, 0));
        textarea.insert_newline();

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );

        textarea.move_cursor(CursorMove::Jump(0, 0));
        textarea.insert_newline();

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_deleting_line_by_head_above_and_below_link() {
        let mut textarea = TextArea::new(
            vec![
                "Some text above".into(),
                "[Link]".into(),
                "Some text below.".into(),
            ],
            HashMap::new(),
            140,
            ta_theme(),
        );

        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false,
            },
        );
        textarea.move_cursor(CursorMove::Jump(2, 0));
        textarea.delete_line_by_head();

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );

        textarea.move_cursor(CursorMove::Jump(0, 0));
        textarea.delete_line_by_head();

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_deleting_line_by_end_above_and_below_link() {
        let mut textarea = TextArea::new(
            vec![
                "Some text above".into(),
                "[Link]".into(),
                "Some text below.".into(),
            ],
            HashMap::new(),
            140,
            ta_theme(),
        );

        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false,
            },
        );
        textarea.move_cursor(CursorMove::Jump(2, 0));
        textarea.move_cursor(CursorMove::End);
        textarea.delete_line_by_end();

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );

        textarea.move_cursor(CursorMove::Jump(0, 0));
        textarea.move_cursor(CursorMove::End);
        textarea.delete_line_by_end();

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_link_full_row_selection_deletion() {
        let mut textarea = TextArea::new(
            vec!["Before [link] after".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.selection_start = Some((0, 0));
        textarea.move_cursor(CursorMove::End);
        textarea.delete_selection(false);

        assert_eq!(textarea.links.get(&0), None);
    }

    #[test]
    fn test_link_full_link_selection_deletion() {
        let mut textarea = TextArea::new(
            vec!["Before [link] after".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.selection_start = Some((0, 7));
        textarea.move_cursor(CursorMove::Jump(0, 12));
        textarea.delete_selection(false);

        assert_eq!(textarea.links.get(&0), None);
    }

    #[test]
    fn test_link_partial_link_selection_deletion() {
        let mut textarea = TextArea::new(
            vec!["Before [link] after".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.selection_start = Some((0, 6));
        textarea.move_cursor(CursorMove::Jump(0, 8));
        textarea.delete_selection(false);

        assert_eq!(textarea.links.get(&0), None);
    }

    #[test]
    fn test_link_shift_after_selection_deletion_same_row() {
        let mut textarea = TextArea::new(
            vec!["Before [link] after".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.selection_start = Some((0, 0));
        textarea.move_cursor(CursorMove::Jump(0, 5));
        textarea.delete_selection(false);

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 2,
                end_col: 7,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_link_shift_after_selection_deletion_multiple_rows_above() {
        let mut textarea = TextArea::new(
            vec![
                "Text above 1".into(),
                "Text above 2".into(),
                "Before [link] after".into(),
            ],
            HashMap::new(),
            140,
            ta_theme(),
        );

        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 2,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.selection_start = Some((0, 0));
        textarea.move_cursor(CursorMove::Jump(1, 11));
        textarea.delete_selection(false);

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_link_shift_after_selection_deletion_above_up_to_link_row_and_start_col() {
        let mut textarea = TextArea::new(
            vec![
                "Text above 1".into(),
                "Text above 2".into(),
                "Before [link] after".into(),
            ],
            HashMap::new(),
            140,
            ta_theme(),
        );

        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 2,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.selection_start = Some((0, 0));
        textarea.move_cursor(CursorMove::Jump(2, 6));
        textarea.delete_selection(false);

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_link_shift_after_selection_deletion_below_from_link_row_and_end_col() {
        let mut textarea = TextArea::new(
            vec![
                "Before [link] after".into(),
                "Text below 1".into(),
                "Text below 2".into(),
            ],
            HashMap::new(),
            140,
            ta_theme(),
        );

        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false,
            },
        );

        textarea.selection_start = Some((0, 13));
        textarea.move_cursor(CursorMove::Jump(2, 11));
        textarea.delete_selection(false);

        assert_eq!(
            textarea.links.get(&0),
            Some(&Link {
                id: 0,
                row: 0,
                start_col: 7,
                end_col: 12,
                edited: false,
                deleted: false
            })
        );
    }

    #[test]
    fn test_delete_newline_no_links() {
        let mut textarea = TextArea::new(
            vec!["Line 1".into(), "Line 2".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.cursor = (1, 0);

        assert!(textarea.delete_newline());
        assert_eq!(textarea.lines, vec!["Line 1Line 2".to_string()]);
    }

    #[test]
    fn test_delete_newline_with_links_next_line() {
        let mut textarea = TextArea::new(
            vec!["Line 1".into(), "[Link]".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 1,
                start_col: 0,
                end_col: 5,
                edited: false,
                deleted: false,
            },
        );
        textarea.cursor = (1, 0);

        assert!(textarea.delete_newline());
        assert_eq!(
            textarea.links.get(&0).unwrap(),
            &Link {
                id: 0,
                row: 0,
                start_col: 5,
                end_col: 10,
                edited: false,
                deleted: false
            }
        );
        assert_eq!(textarea.lines, vec!["Line 1[Link]".to_string()]);
    }

    #[test]
    fn test_delete_newline_with_links_both_lines() {
        let mut textarea = TextArea::new(
            vec!["[Link1]".into(), "[Link2]".into()],
            HashMap::new(),
            140,
            ta_theme(),
        );
        textarea.links.insert(
            0,
            Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 6,
                edited: false,
                deleted: false,
            },
        );
        textarea.links.insert(
            1,
            Link {
                id: 1,
                row: 1,
                start_col: 0,
                end_col: 6,
                edited: false,
                deleted: false,
            },
        );
        textarea.cursor = (1, 0);

        assert!(textarea.delete_newline());
        assert_eq!(
            textarea.links.get(&0).unwrap(),
            &Link {
                id: 0,
                row: 0,
                start_col: 0,
                end_col: 6,
                edited: false,
                deleted: false
            }
        );
        assert_eq!(
            textarea.links.get(&1).unwrap(),
            &Link {
                id: 1,
                row: 0,
                start_col: 7,
                end_col: 13,
                edited: false,
                deleted: false
            }
        );
        assert_eq!(textarea.lines, vec!["[Link1][Link2]".to_string()]);
    }
}
