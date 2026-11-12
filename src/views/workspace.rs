//! Main resizable layout container organizing file tree, center editor/diff, terminal, and chat panels.

use freya::prelude::*;

/// Builds the resizable workspace layout. When `companion_mode` is active,
/// hides the terminal panel to give full vertical space to the Diff Review & Audit Cockpit.
pub fn main_workspace_layout(
    file_tree: impl IntoElement,
    center_panel: impl IntoElement,
    terminal: impl IntoElement,
    chat: impl IntoElement,
    companion_mode: bool,
    show_sidebar: bool,
    show_dashboard: bool,
) -> impl IntoElement {
    if !show_dashboard {
        if show_sidebar {
            return ResizableContainer::new()
                .direction(Direction::Horizontal)
                .panel(ResizablePanel::new(PanelSize::percent(25.)).child(file_tree))
                .panel(
                    ResizablePanel::new(PanelSize::percent(75.))
                        .child(rect().expanded().content(Content::Flex).child(chat)),
                )
                .into_element();
        } else {
            return rect()
                .expanded()
                .content(Content::Flex)
                .child(chat)
                .into_element();
        }
    }

    if show_sidebar {
        if companion_mode {
            ResizableContainer::new()
                .direction(Direction::Horizontal)
                .panel(ResizablePanel::new(PanelSize::percent(22.)).child(file_tree))
                .panel(ResizablePanel::new(PanelSize::percent(46.)).child(center_panel))
                .panel(
                    ResizablePanel::new(PanelSize::percent(32.))
                        .child(rect().expanded().content(Content::Flex).child(chat)),
                )
                .into_element()
        } else {
            ResizableContainer::new()
                .direction(Direction::Horizontal)
                .panel(ResizablePanel::new(PanelSize::percent(18.)).child(file_tree))
                .panel(
                    ResizablePanel::new(PanelSize::percent(52.)).child(
                        ResizableContainer::new()
                            .direction(Direction::Vertical)
                            .panel(ResizablePanel::new(PanelSize::percent(55.)).child(center_panel))
                            .panel(ResizablePanel::new(PanelSize::percent(45.)).child(terminal)),
                    ),
                )
                .panel(
                    ResizablePanel::new(PanelSize::percent(30.))
                        .child(rect().expanded().content(Content::Flex).child(chat)),
                )
                .into_element()
        }
    } else {
        if companion_mode {
            ResizableContainer::new()
                .direction(Direction::Horizontal)
                .panel(ResizablePanel::new(PanelSize::percent(64.)).child(center_panel))
                .panel(
                    ResizablePanel::new(PanelSize::percent(36.))
                        .child(rect().expanded().content(Content::Flex).child(chat)),
                )
                .into_element()
        } else {
            ResizableContainer::new()
                .direction(Direction::Horizontal)
                .panel(
                    ResizablePanel::new(PanelSize::percent(64.)).child(
                        ResizableContainer::new()
                            .direction(Direction::Vertical)
                            .panel(ResizablePanel::new(PanelSize::percent(55.)).child(center_panel))
                            .panel(ResizablePanel::new(PanelSize::percent(45.)).child(terminal)),
                    ),
                )
                .panel(
                    ResizablePanel::new(PanelSize::percent(36.))
                        .child(rect().expanded().content(Content::Flex).child(chat)),
                )
                .into_element()
        }
    }
}
