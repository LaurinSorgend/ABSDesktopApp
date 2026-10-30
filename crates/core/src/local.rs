//! Books in folders on this computer, recognised by the rules of the ABS scanner.

mod group;
mod names;
mod order;

pub use group::{BookGroup, FileKind, group_files, kind_of};
pub use names::{FolderData, folder_data, parse_names, title_ignore_prefix};
pub use order::{AudioFacts, chapters, smart_order};
