#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop)]
#![warn(clippy::dbg_macro, clippy::print_stdout)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

mod cursor;
mod highlight;
mod history;
mod hop;
mod input;
mod links;
mod scroll;
#[cfg(feature = "search")]
mod search;
mod textarea;
mod textinput;
mod util;
mod widget;
mod word;

#[cfg(feature = "ratatui")]
#[allow(clippy::single_component_path_imports)]
use ratatui;

#[cfg(feature = "crossterm")]
#[allow(clippy::single_component_path_imports)]
use crossterm;

pub use cursor::CursorMove;
pub use input::{Input, Key};
pub use links::Link;
pub use scroll::Scrolling;
pub use textarea::TextArea;
pub use textarea::TextAreaTheme;
pub use textinput::TextInput;
