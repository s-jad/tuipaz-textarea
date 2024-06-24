use ratatui::{widgets::{Block, Widget, Paragraph}, style::{Style, Color, Modifier}, layout::Alignment, text::Text};
use unicode_width::UnicodeWidthChar;

use crate::{Input, Key, word::{find_word_start_backward, find_word_end_forward}, util::spaces};

#[derive(Clone, Debug)]
pub struct TextInput<'a> {
    text: String,
    block: Option<Block<'a>>,
    style: Style,
    pub cursor: (usize, usize), // 0-base
    tab_len: u8,
    alignment: Alignment,
    pub(crate) placeholder: String,
    pub(crate) placeholder_style: Style,
    cursor_style: Style,
    max_col: u16,
}

impl<'a> TextInput<'a> {
    pub fn new(text: String, max_col: u16, text_clr: Color, placeholder: String) -> Self {
        let style = Style::new().fg(text_clr);

        Self {
            text,
            block: None,
            style,
            cursor: (0, 0),
            tab_len: 4,
            alignment: Alignment::Left,
            placeholder,
            placeholder_style: style,
            cursor_style: Style::default().add_modifier(Modifier::REVERSED),
            max_col,
        }
    }

    pub fn input(&mut self, input: impl Into<Input>) -> bool {
        let input = input.into();
        match input {
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
                ctrl: false,
                alt: false,
                ..
            } => self.delete_char(),
            Input {
                key: Key::Delete,
                ctrl: false,
                alt: false,
                ..
            } => self.delete_next_char(),
            Input {
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
            } => self.delete_next_word(),
            _ => false,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let (_, col) = self.cursor;
        let line = &mut self.text;
        let i = line
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        line.insert(i, c);
        self.cursor.1 += 1;
    }

    pub fn delete_char(&mut self) -> bool {
        let (_, col) = self.cursor;
        if col == 0 {
            return false;
        }
        let line = &mut self.text;
        if let Some((offset, _)) = line.char_indices().nth(col - 1) {
            line.remove(offset);
            self.cursor.1 = self.cursor.1.saturating_sub(1);
            true
        } else {
            false
        }
    }

    pub fn delete_next_char(&mut self) -> bool {
        let (row, col) = self.cursor;
        if col + 1 >= self.text.len() {
            return false;
        }
        self.cursor = (row, col + 1);
        self.delete_char()
    }

    pub fn delete_word(&mut self) -> bool {
        let (_, col) = self.cursor;
        if let Some(word_start) = find_word_start_backward(&self.text, col) {
            self.text.drain(word_start..=col);
            true
        } else if col > 0 {
            self.text.drain(0..=col);
            true
        } else {
            false
        }
    }

    pub fn delete_next_word(&mut self) -> bool {
        let (_, start_col) = self.cursor;
        let line = &self.text;
        if let Some(word_end) = find_word_end_forward(line, start_col) {
            self.text.drain(start_col..=word_end);
            true
        } else {
            let line_end = line.chars().count();
            if start_col < line_end {
                self.text.drain(start_col..=line_end);
                true
            } else {
                false
            }
        }
    }

    pub fn insert_tab(&mut self) -> bool {
        if self.tab_len == 0 {
            return false;
        }

        let (_, col) = self.cursor;
        let width: usize = self.text
            .chars()
            .take(col)
            .map(|c| c.width().unwrap_or(0))
            .sum();
        let len = self.tab_len - (width % self.tab_len as usize) as u8;
        self.text.insert_str(col, spaces(len));
        true
    }

    pub fn clear(&mut self) {
        self.text = "".to_owned();
    }

    pub fn get_text(&self) -> &str {
        &self.text
    }

    pub fn set_block(&mut self, block: Block<'a>) {
        self.block = Some(block);
    }

    pub fn set_placeholder_text(&mut self, placeholder: &str) {
        self.placeholder = placeholder.to_owned();
    }

    pub fn set_cursor_style(&mut self, style: Style) {
        self.cursor_style = style;
    }

    pub fn block<'s>(&'s self) -> Option<&'s Block<'a>> {
        self.block.as_ref()
    }

    pub fn text(&self) -> &str {
        self.text.as_ref()
    }

    pub fn widget(&'a self) -> impl Widget + 'a {
        Renderer::new(self)
    }
}

pub struct Renderer<'a>(&'a TextInput<'a>);

impl<'a> Renderer<'a> {
    pub fn new(textarea: &'a TextInput<'a>) -> Self {
        Self(textarea)
    }
}

impl<'a> Widget for Renderer<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where Self: Sized 
    {
        let (text, style) = if !self.0.placeholder.is_empty() && self.0.text.is_empty() {
            let text = Text::from(self.0.placeholder.as_str());
            (text, self.0.placeholder_style)
        } else {
            (Text::from(self.0.text()), self.0.style)
        };

        let inner = Paragraph::new(text)
            .style(style)
            .alignment(Alignment::Left);
        
        let mut text_input = area;
        if let Some(b) = self.0.block() {
            text_input = b.inner(area);
            b.clone().render(area, buf)
        }

        inner.render(text_input, buf);
    }
}
