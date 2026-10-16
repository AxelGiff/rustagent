//! Main resizable layout container organizing file tree, center editor/diff, terminal, and chat panels.

use freya::prelude::*;

/// Builds the 3-column resizable main workspace layout.
pub fn main_workspace_layout(
    file_tree: impl IntoElement,
    center_panel: impl IntoElement,
    terminal: impl IntoElement,
    chat: impl IntoElement,
) -> impl IntoElement {
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
}
