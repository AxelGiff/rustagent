//! UI components and panel views for RustAgent.

pub mod chat;
pub mod diff_viewer;
pub mod editor;
pub mod file_tree;
pub mod settings;
pub mod terminal;
pub mod toolbar;
pub mod workspace;

pub use chat::chat_panel;
pub use diff_viewer::diff_viewer_panel;
pub use editor::{code_editor_panel, create_editor_data, update_editor_if_matching};
pub use file_tree::file_tree_panel;
pub use settings::settings_page_view;
pub use terminal::{spawn_terminal, terminal_panel};
pub use toolbar::app_toolbar;
pub use workspace::main_workspace_layout;
