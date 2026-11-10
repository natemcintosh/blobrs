use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph, Widget, Wrap},
};

use crate::app::{App, BlobInfo};

use super::{
    ACCENT, BACKGROUND, DANGER, MUTED, SURFACE, TEXT,
    command_bar::{CommandBar, CommandHint as Hint},
    detail, format_bytes, panel, truncate_with_ellipsis,
};

/// Clear the underlying view and reserve space for the same boxed shortcuts as
/// the main command bar. The returned body stays inside the padded panel.
fn popup(
    area: Rect,
    buf: &mut Buffer,
    title: &str,
    width: u16,
    body_height: u16,
    hints: &[Hint],
) -> Rect {
    let width = width.min(area.width);
    let commands = CommandBar::new(hints, width.saturating_sub(4), None);
    let height = body_height
        .saturating_add(commands.height())
        .saturating_add(5) // Borders, padding, and a gap before the shortcuts.
        .min(area.height);
    let popup_area = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    Clear.render(popup_area, buf);
    let block = panel()
        .title(format!(" {title} "))
        .padding(Padding::uniform(1))
        .style(Style::default().fg(TEXT).bg(BACKGROUND));
    let inner = block.inner(popup_area);
    block.render(popup_area, buf);
    let [body, _, footer] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(commands.height()),
    ])
    .areas(inner);
    commands.render(footer, buf);
    body
}

/// Show the end of an input without slicing UTF-8 or hiding the cursor. Ratatui
/// measures terminal columns, so wide characters also fit within the field.
fn input_field(area: Rect, buf: &mut Buffer, input: &str) {
    if area.is_empty() {
        return;
    }
    let available = usize::from(area.width.saturating_sub(1));
    let mut visible = input;
    let mut scrolled = false;
    while Line::from(visible).width() + usize::from(scrolled) > available {
        let Some(first) = visible.chars().next() else {
            break;
        };
        visible = &visible[first.len_utf8()..];
        scrolled = true;
    }
    let prefix = if scrolled && available > 0 { "…" } else { "" };
    Paragraph::new(Line::from(vec![
        Span::raw(prefix),
        Span::raw(visible),
        Span::styled(
            "▏",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]))
    .style(Style::default().fg(TEXT).bg(SURFACE))
    .render(area, buf);
}

impl App {
    pub(super) fn render_blob_info_popup(&self, area: Rect, buf: &mut Buffer, info: &BlobInfo) {
        let value_width = usize::from(area.width.min(64).saturating_sub(14));
        let lines = match info {
            BlobInfo::Folder {
                name,
                blob_count,
                total_size,
            } => vec![
                Line::styled(
                    format!("{} Folder", self.icons.folder),
                    Style::default().fg(ACCENT),
                ),
                Line::default(),
                detail("Name", truncate_with_ellipsis(name, value_width)),
                Line::default(),
                detail("Blobs", blob_count.to_string()),
                detail("Total size", format_bytes(*total_size)),
            ],
            BlobInfo::File {
                name,
                size,
                last_modified,
                etag,
            } => {
                let mut lines = vec![
                    Line::styled(
                        format!("{} Blob", self.icons.file),
                        Style::default().fg(ACCENT),
                    ),
                    Line::default(),
                    detail("Name", truncate_with_ellipsis(name, value_width)),
                    Line::default(),
                    detail("Size", format_bytes(*size)),
                    detail("Modified", last_modified.clone()),
                ];
                if let Some(etag) = etag {
                    lines.push(detail("ETag", truncate_with_ellipsis(etag, value_width)));
                }
                lines
            }
        };
        let body = popup(
            area,
            buf,
            "Information",
            64,
            9,
            &[Hint::new("Esc/←/h", "Close")],
        );
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(body, buf);
    }

    pub(super) fn render_upload_picker_popup(&self, area: Rect, buf: &mut Buffer) {
        let Some(browsing) = self.browsing() else {
            return;
        };
        let body = popup(
            area,
            buf,
            "Upload",
            68,
            7,
            &[
                Hint::new("Enter", "Choose file"),
                Hint::new("Esc", "Cancel"),
            ],
        );
        Paragraph::new(vec![
            detail("Destination", format!("/{}", browsing.current_path())),
            Line::default(),
            Line::from("The file keeps its name. Existing blobs are not replaced."),
            Line::default(),
            Line::styled(
                "To upload elsewhere, cancel and navigate to that folder first.",
                Style::default().fg(MUTED),
            ),
        ])
        .wrap(Wrap { trim: false })
        .render(body, buf);
    }

    pub(super) fn render_download_picker_popup(&self, area: Rect, buf: &mut Buffer) {
        let Some(browsing) = self.browsing() else {
            return;
        };
        let selected = browsing
            .files()
            .get(browsing.selected_index)
            .map_or("No file selected", String::as_str);
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);
        let name = selected
            .strip_prefix(&folder_prefix)
            .or_else(|| selected.strip_prefix(&file_prefix))
            .unwrap_or(selected);
        let body = popup(
            area,
            buf,
            "Download",
            68,
            4,
            &[
                Hint::new("Enter", "Choose destination"),
                Hint::new("Esc", "Cancel"),
            ],
        );
        Paragraph::new(vec![
            detail("Download", name.to_string()),
            Line::default(),
            Line::styled(
                "Choose a local folder for the download.",
                Style::default().fg(MUTED),
            ),
        ])
        .wrap(Wrap { trim: false })
        .render(body, buf);
    }

    pub(super) fn render_sort_popup(area: Rect, buf: &mut Buffer) {
        let body = popup(
            area,
            buf,
            "Sort files",
            64,
            4,
            &[
                Hint::new("n", "Name"),
                Hint::new("m", "Modified"),
                Hint::new("t", "Created"),
                Hint::new("s", "Size"),
                Hint::new("Esc", "Cancel"),
            ],
        );
        Paragraph::new(vec![
            Line::from("Choose how to order files and folders."),
            Line::default(),
            Line::styled(
                "Date and size sorts scan all blobs in folders.",
                Style::default().fg(MUTED),
            ),
        ])
        .wrap(Wrap { trim: false })
        .render(body, buf);
    }

    pub(super) fn render_clone_dialog_popup(
        area: Rect,
        buf: &mut Buffer,
        input: &str,
        original_path: &str,
        is_folder: bool,
    ) {
        let item_type = if is_folder { "folder" } else { "blob" };
        let original = detail("Original", original_path.to_string());
        let content_width = usize::from(area.width.min(72).saturating_sub(4)).max(1);
        let path_height =
            u16::try_from(original.width().div_ceil(content_width)).unwrap_or(u16::MAX);
        let body = popup(
            area,
            buf,
            "Clone",
            72,
            path_height.saturating_add(6),
            &[Hint::new("Enter", "Clone"), Hint::new("Esc", "Cancel")],
        );
        let [description, label, field, _, note] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(body);
        Paragraph::new(vec![
            Line::from(format!("Clone {item_type} to a new path.")),
            Line::default(),
            original,
        ])
        .wrap(Wrap { trim: false })
        .render(description, buf);
        Paragraph::new("New path")
            .style(Style::default().fg(MUTED))
            .render(label, buf);
        input_field(field, buf, input);
        let hint = if input == original_path || input.is_empty() {
            "Change the path before confirming."
        } else {
            "The original stays in place."
        };
        Paragraph::new(hint)
            .style(Style::default().fg(MUTED))
            .render(note, buf);
    }

    pub(super) fn render_delete_dialog_popup(
        area: Rect,
        buf: &mut Buffer,
        input: &str,
        target_name: &str,
        is_folder: bool,
    ) {
        let body = popup(
            area,
            buf,
            "Delete",
            72,
            9,
            &[Hint::new("Enter", "Delete"), Hint::new("Esc", "Cancel")],
        );
        let [description, label, field, _, note] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(body);
        let item_type = if is_folder { "folder" } else { "blob" };
        let warning = if is_folder {
            "All blobs in this folder will be deleted. This cannot be undone."
        } else {
            "This action cannot be undone."
        };
        Paragraph::new(vec![
            detail(&format!("Delete {item_type}"), target_name.to_string()),
            Line::default(),
            Line::styled(warning, Style::default().fg(DANGER)),
        ])
        .wrap(Wrap { trim: false })
        .render(description, buf);
        Paragraph::new("Type the exact name to confirm")
            .style(Style::default().fg(MUTED))
            .render(label, buf);
        input_field(field, buf, input);
        let (hint, color) = if input == target_name {
            ("Name matches. Press Enter to delete permanently.", DANGER)
        } else {
            ("The name must match before you can delete.", MUTED)
        };
        Paragraph::new(hint)
            .style(Style::default().fg(color))
            .render(note, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogs_stay_inside_small_and_offset_viewports() {
        for width in [0, 1, 2, 4, 12, 24, 40, 80] {
            for height in [0, 1, 2, 4, 8, 16, 30] {
                let area = Rect::new(3, 2, width, height);
                let mut buf = Buffer::empty(area);
                App::render_sort_popup(area, &mut buf);
                App::render_clone_dialog_popup(
                    area,
                    &mut buf,
                    "新しい名前🦀.csv",
                    "original.csv",
                    false,
                );
                App::render_delete_dialog_popup(area, &mut buf, "résumé.csv", "résumé.csv", false);
                let body = popup(
                    area,
                    &mut buf,
                    "Information",
                    64,
                    9,
                    &[Hint::new("Esc", "Close")],
                );
                assert_eq!(body.intersection(area), body);
            }
        }
    }

    #[test]
    fn input_scrolls_unicode_and_keeps_the_cursor_visible() {
        let input = "folder/résumé/新しい名前🦀.csv";
        for width in 1..40 {
            let area = Rect::new(2, 3, width, 1);
            let mut buf = Buffer::empty(area);
            input_field(area, &mut buf, input);
            let rendered: String = buf
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect();
            assert!(
                rendered.contains('▏'),
                "cursor missing at width {width}: {rendered}"
            );
            if width > 1 && usize::from(width) <= Line::from(input).width() {
                assert!(rendered.starts_with('…'), "{rendered}");
            }
        }
    }
}
