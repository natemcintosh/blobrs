use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Widget},
};

use crate::app::{App, Modal, Session};

use super::{ACCENT, MUTED, SURFACE, TEXT};

/// A shortcut stays together when the command bar wraps to another row.
#[derive(Clone, Copy)]
struct CommandHint {
    key: &'static str,
    label: &'static str,
}

impl CommandHint {
    const fn new(key: &'static str, label: &'static str) -> Self {
        Self { key, label }
    }

    fn spans(self) -> [Span<'static>; 3] {
        [
            Span::styled(
                format!(" {} ", self.key),
                Style::default()
                    .fg(ACCENT)
                    .bg(SURFACE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {}", self.label), Style::default().fg(TEXT)),
            Span::raw("  "),
        ]
    }
}

pub(super) struct CommandBar {
    commands: Vec<Line<'static>>,
    status: Option<String>,
}

impl CommandBar {
    fn new(hints: &[CommandHint], width: u16, status: Option<String>) -> Self {
        let mut commands = Vec::new();
        let mut line = Line::default();
        for hint in hints {
            let spans = hint.spans();
            let hint_width: usize = spans.iter().map(Span::width).sum();
            if !line.spans.is_empty() && line.width() + hint_width > usize::from(width) {
                commands.push(line);
                line = Line::default();
            }
            line.spans.extend(spans);
        }
        commands.push(line);
        Self { commands, status }
    }

    pub(super) fn height(&self) -> u16 {
        u16::try_from(self.commands.len() + usize::from(self.status.is_some())).unwrap_or(u16::MAX)
    }
}

impl Widget for CommandBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let status_height = u16::from(self.status.is_some()).min(area.height);
        if let Some(status) = self.status {
            Paragraph::new(status)
                .style(Style::default().fg(MUTED))
                .render(
                    Rect {
                        height: status_height,
                        ..area
                    },
                    buf,
                );
        }
        Paragraph::new(Text::from(self.commands)).render(
            Rect {
                y: area.y.saturating_add(status_height),
                height: area.height.saturating_sub(status_height),
                ..area
            },
            buf,
        );
    }
}

impl App {
    pub(super) fn command_bar(&self, width: u16) -> CommandBar {
        use CommandHint as Hint;

        let hints: &[CommandHint] = if self.tasks.is_running() {
            &[Hint::new("Esc/Ctrl-C", "Cancel"), Hint::new("q", "Quit")]
        } else if self.is_searching_containers() || self.is_searching_files() {
            &[
                Hint::new("Enter", "Apply"),
                Hint::new("Esc", "Cancel"),
                Hint::new("Ctrl+↑/↓", "Navigate"),
            ]
        } else {
            match &self.modal {
                Modal::Clone { .. } | Modal::DeleteConfirm { .. } => {
                    &[Hint::new("Enter", "Confirm"), Hint::new("Esc", "Cancel")]
                }
                Modal::UploadPicker => &[
                    Hint::new("Enter", "Choose file"),
                    Hint::new("Esc", "Cancel"),
                    Hint::new("q", "Quit"),
                ],
                Modal::DownloadPicker { .. } => &[
                    Hint::new("Enter", "Choose destination"),
                    Hint::new("Esc", "Cancel"),
                    Hint::new("q", "Quit"),
                ],
                Modal::SortPicker => &[
                    Hint::new("n", "Name"),
                    Hint::new("m", "Modified"),
                    Hint::new("t", "Created"),
                    Hint::new("s", "Size"),
                    Hint::new("Esc", "Cancel"),
                ],
                Modal::BlobInfo { .. } => &[Hint::new("Esc/←/h", "Close"), Hint::new("q", "Quit")],
                Modal::None if self.ui.show_preview => {
                    let mut preview = vec![
                        Hint::new("↑/↓/k/j", "Rows"),
                        Hint::new("←/→/h/l", "Columns"),
                    ];
                    if self.preview_file_type == Some(crate::preview::PreviewFileType::Parquet) {
                        preview.push(Hint::new("Tab", "Table/metadata"));
                    }
                    preview.extend([Hint::new("p/Esc", "Close"), Hint::new("q", "Quit")]);
                    return CommandBar::new(&preview, width, None);
                }
                Modal::None => match self.session {
                    Session::Selecting => &[
                        Hint::new("↑/↓/k/j", "Navigate"),
                        Hint::new("Enter/→/l", "Open"),
                        Hint::new("/", "Search"),
                        Hint::new("f", "Favorite"),
                        Hint::new("r/F5", "Refresh"),
                        Hint::new("q/Esc", "Quit"),
                    ],
                    Session::Browsing(_) => &[
                        Hint::new("↑/↓/k/j", "Navigate"),
                        Hint::new("Enter/→/l", "Open"),
                        Hint::new("Esc/←/h", "Back"),
                        Hint::new("/", "Search"),
                        Hint::new("s", "Sort"),
                        Hint::new("i", "Info"),
                        Hint::new("p", "Preview"),
                        Hint::new("y", "Copy path"),
                        Hint::new("d", "Download"),
                        Hint::new("u", "Upload"),
                        Hint::new("c", "Clone"),
                        Hint::new("x", "Delete"),
                        Hint::new("r/F5", "Refresh"),
                        Hint::new("q", "Quit"),
                    ],
                },
            }
        };
        CommandBar::new(hints, width, self.tasks.status())
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandBar, CommandHint};
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    #[test]
    fn shortcuts_wrap_as_complete_groups_using_terminal_width() {
        let hints = [
            CommandHint::new("↑/↓/k/j", "Navigate"),
            CommandHint::new("Enter/→/l", "Open"),
            CommandHint::new("/", "Search"),
            CommandHint::new("q", "Quit"),
        ];
        for width in [24, 40, 80, 120] {
            let bar = CommandBar::new(&hints, width, None);
            assert!(
                bar.commands
                    .iter()
                    .all(|line| line.width() <= usize::from(width))
            );
            let area = Rect::new(3, 2, width, bar.height());
            let mut buffer = Buffer::empty(area);
            bar.render(area, &mut buffer);
            let text: String = buffer
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect();
            for hint in hints {
                assert!(
                    text.contains(&format!(" {}  {}", hint.key, hint.label)),
                    "{text}"
                );
            }
        }
    }

    #[test]
    fn command_bar_handles_tiny_and_empty_areas() {
        for width in 0..24 {
            for height in 0..3 {
                let bar = CommandBar::new(
                    &[
                        CommandHint::new("Esc/Ctrl-C", "Cancel"),
                        CommandHint::new("q", "Quit"),
                    ],
                    width,
                    Some("Loading files".into()),
                );
                let area = Rect::new(0, 0, width, height);
                bar.render(area, &mut Buffer::empty(area));
            }
        }
    }
}
