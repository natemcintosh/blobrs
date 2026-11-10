use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::app::{App, Modal, PreviewPanel, Session};

use super::{ACCENT, MUTED, SURFACE, TEXT, panel};

const COMMAND_ROW_HEIGHT: u16 = 3;
const COMMAND_GAP: usize = 1;

/// A shortcut stays together when the command bar wraps to another row.
#[derive(Clone, Copy)]
pub(super) struct CommandHint {
    key: &'static str,
    label: &'static str,
}

impl CommandHint {
    pub(super) const fn new(key: &'static str, label: &'static str) -> Self {
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
            Span::raw(" "),
        ]
    }
}

pub(super) struct CommandBar {
    commands: Vec<Vec<Line<'static>>>,
    status: Option<String>,
}

impl CommandBar {
    pub(super) fn new(hints: &[CommandHint], width: u16, status: Option<String>) -> Self {
        let mut commands = Vec::new();
        let mut row = Vec::new();
        let mut row_width = 0;
        for hint in hints {
            let line = Line::from(Vec::from(hint.spans()));
            let hint_width = line.width() + 2; // Left and right borders.
            if !row.is_empty() && row_width + COMMAND_GAP + hint_width > usize::from(width) {
                commands.push(row);
                row = Vec::new();
                row_width = 0;
            }
            if !row.is_empty() {
                row_width += COMMAND_GAP;
            }
            row_width += hint_width;
            row.push(line);
        }
        if !row.is_empty() {
            commands.push(row);
        }
        Self { commands, status }
    }

    pub(super) fn height(&self) -> u16 {
        u16::try_from(self.commands.len())
            .unwrap_or(u16::MAX)
            .saturating_mul(COMMAND_ROW_HEIGHT)
            .saturating_add(u16::from(self.status.is_some()))
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
        let mut y = area.y.saturating_add(status_height);
        for row in self.commands {
            if y >= area.bottom() {
                break;
            }
            let mut x = area.x;
            for command in row {
                let width = u16::try_from(command.width() + 2)
                    .unwrap_or(u16::MAX)
                    .min(area.right().saturating_sub(x));
                let command_area = Rect::new(
                    x,
                    y,
                    width,
                    COMMAND_ROW_HEIGHT.min(area.bottom().saturating_sub(y)),
                );
                Paragraph::new(command)
                    .block(panel())
                    .render(command_area, buf);
                x = x.saturating_add(width).saturating_add(1);
            }
            y = y.saturating_add(COMMAND_ROW_HEIGHT);
        }
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
                Modal::None if self.ui.preview == PreviewPanel::Focused => {
                    let mut preview = vec![
                        Hint::new("↑/↓/k/j", "Rows"),
                        Hint::new("←/→/h/l", "Columns"),
                        Hint::new("Home/End", "First/last"),
                    ];
                    if self.preview_scope == crate::preview::PreviewScope::Limited
                        && self
                            .preview_file_type
                            .as_ref()
                            .is_some_and(crate::preview::PreviewFileType::is_supported)
                    {
                        preview.push(Hint::new("f", "Full preview"));
                    }
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
        if matches!(self.session, Session::Browsing(_))
            && matches!(self.modal, Modal::None)
            && !self.is_searching_files()
            && !self.tasks.is_running()
            && self.ui.preview == PreviewPanel::Browsing
            && self.preview_file_type == Some(crate::preview::PreviewFileType::Parquet)
        {
            let mut hints = hints.to_vec();
            hints.push(Hint::new("Tab", "Table/metadata"));
            return CommandBar::new(&hints, width, None);
        }
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
                bar.commands.iter().all(|row| row
                    .iter()
                    .map(|line| line.width() + 2)
                    .sum::<usize>()
                    + row.len().saturating_sub(1)
                    <= usize::from(width))
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
                    text.contains(&format!("│ {}  {} │", hint.key, hint.label)),
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
