use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState,
        Widget, Wrap,
    },
};

use crate::app::{App, Modal, ParquetPreviewMode, Session};
use crate::preview::PreviewData;

mod command_bar;
mod popups;

const TEXT: Color = Color::Rgb(210, 218, 228);
const MUTED: Color = Color::Rgb(145, 157, 174);
const BORDER: Color = Color::Rgb(80, 94, 112);
const ACCENT: Color = Color::Rgb(125, 195, 232);
const SURFACE: Color = Color::Rgb(36, 49, 65);

fn panel() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
}

fn selection() -> Style {
    Style::default()
        .bg(SURFACE)
        .fg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

impl Widget for &App {
    /// Renders the user interface widgets.
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_style(area, Style::default().fg(TEXT));
        match &self.session {
            Session::Selecting => self.render_container_selection(area, buf),
            Session::Browsing(_) => {
                self.render_blob_browsing(area, buf);

                // Render popup over the blob browsing view if needed
                match &self.modal {
                    Modal::DeleteConfirm {
                        input,
                        target_name,
                        is_folder,
                        ..
                    } => {
                        App::render_delete_dialog_popup(area, buf, input, target_name, *is_folder);
                    }
                    Modal::Clone {
                        input,
                        original_path,
                        is_folder,
                    } => {
                        App::render_clone_dialog_popup(area, buf, input, original_path, *is_folder);
                    }
                    Modal::BlobInfo { info } => {
                        self.render_blob_info_popup(area, buf, info);
                    }
                    Modal::DownloadPicker { .. } => {
                        self.render_download_picker_popup(area, buf);
                    }
                    Modal::UploadPicker => self.render_upload_picker_popup(area, buf),
                    Modal::SortPicker => {
                        App::render_sort_popup(area, buf);
                    }
                    Modal::None => {}
                }
            }
        }
    }
}

impl App {
    #[allow(clippy::too_many_lines)]
    fn render_container_selection(&self, area: Rect, buf: &mut Buffer) {
        let command_bar = self.command_bar(area.width);
        let footer_height = command_bar.height();

        // Create a vertical layout
        let mut constraints = vec![
            Constraint::Min(0), // Main content area
        ];

        // Add space for search input if in container search mode
        if self.is_searching_containers() {
            constraints.push(Constraint::Length(3)); // Search input area
        }

        // Add space for error or success message if present
        if self.error_message.is_some() || self.success_message.is_some() {
            // Calculate height based on message length and terminal width
            #[allow(clippy::cast_possible_truncation)] // UI text lengths are always small
            let message_height = if let Some(error) = &self.error_message {
                // Estimate lines needed: error length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((error.len() + format!("{error_icon} ", error_icon = self.icons.error).len())
                        as u16)
                        .div_ceil(available_width)
                        .saturating_add(2) // Include the top and bottom borders.
                        .clamp(3, 8)
                } else {
                    3
                }
            } else if let Some(success) = &self.success_message {
                // Estimate lines needed: success length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((success.len()
                        + format!("{success_icon} ", success_icon = self.icons.success).len())
                        as u16)
                        .div_ceil(available_width)
                        .saturating_add(2) // Include the top and bottom borders.
                        .clamp(3, 8)
                } else {
                    3
                }
            } else {
                3
            };
            constraints.push(Constraint::Length(message_height)); // Message area
        }

        constraints.push(Constraint::Length(footer_height)); // Footer for instructions

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Container list
        let container_items: Vec<ListItem> = if self.containers.is_empty() {
            let has_query = self.container_search_query().is_some_and(|q| !q.is_empty());
            if self.is_searching_containers() && has_query {
                vec![ListItem::new(format!(
                    "{search} No containers found matching search",
                    search = self.icons.search
                ))]
            } else {
                vec![ListItem::new(format!(
                    "{empty} No containers found",
                    empty = self.icons.empty
                ))]
            }
        } else {
            self.containers
                .iter()
                .map(|container| {
                    let name = &container.name;
                    let favorite = if self.is_favorite(name) { "★" } else { " " };
                    ListItem::new(format!(
                        "{favorite} {folder} {name}",
                        folder = self.icons.folder
                    ))
                })
                .collect()
        };

        let mut list_state = ListState::default();
        if !self.containers.is_empty() {
            list_state.select(Some(self.selected_container_index));
        }

        let title = if self.is_searching_containers() {
            format!(
                " {account} / Containers · Search ({count} shown) ",
                account = self.storage_account,
                count = self.containers.len()
            )
        } else {
            format!(
                " {account} / Containers ({count}) ",
                account = self.storage_account,
                count = self.containers.len()
            )
        };

        let main_block = List::new(container_items)
            .block(
                panel()
                    .title(title)
                    .title_alignment(Alignment::Left)
                    .border_type(BorderType::Rounded),
            )
            .fg(TEXT)
            .highlight_style(selection())
            .highlight_symbol("▸ ");

        ratatui::widgets::StatefulWidget::render(main_block, chunks[0], buf, &mut list_state);

        let mut chunk_index = 1;

        // Search input if in container search mode
        if self.is_searching_containers() {
            let query = self.container_search_query().unwrap_or("");
            let search_text = format!("Search: {query}");
            let search_widget = Paragraph::new(search_text)
                .block(
                    panel()
                        .title(" Filter containers ")
                        .border_type(BorderType::Rounded),
                )
                .fg(Color::Cyan)
                .alignment(Alignment::Left);
            search_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        // Error/success message if present
        if let Some(error) = &self.error_message {
            let error_widget = Paragraph::new(format!(
                "{error_icon} {error}",
                error_icon = self.icons.error
            ))
            .block(panel().border_type(BorderType::Rounded))
            .fg(Color::Red)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .alignment(Alignment::Left);
            error_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        } else if let Some(success) = &self.success_message {
            let success_widget = Paragraph::new(format!(
                "{success_icon} {success}",
                success_icon = self.icons.success
            ))
            .block(panel().border_type(BorderType::Rounded))
            .fg(Color::Green)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .alignment(Alignment::Left);
            success_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        command_bar.render(chunks[chunk_index], buf);
    }

    #[allow(clippy::too_many_lines)]
    fn render_blob_browsing(&self, area: Rect, buf: &mut Buffer) {
        let Some(browsing) = self.browsing() else {
            unreachable!("render_blob_browsing called when session is not Session::Browsing");
        };

        let command_bar = self.command_bar(area.width);
        let footer_height = command_bar.height();

        // Create a vertical layout with main content, search (if active), message, and footer
        let mut constraints = vec![
            Constraint::Min(0), // Main content area
        ];

        // Add space for search input if in search mode
        if self.is_searching_files() {
            constraints.push(Constraint::Length(3)); // Search input area
        }

        // Add space for error or success message if present
        if self.error_message.is_some() || self.success_message.is_some() {
            // Calculate height based on message length and terminal width
            #[allow(clippy::cast_possible_truncation)] // UI text lengths are always small
            let message_height = if let Some(error) = &self.error_message {
                // Estimate lines needed: error length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((error.len() + format!("{error_icon} ", error_icon = self.icons.error).len())
                        as u16)
                        .div_ceil(available_width)
                        .saturating_add(2) // Include the top and bottom borders.
                        .clamp(3, 8)
                } else {
                    3
                }
            } else if let Some(success) = &self.success_message {
                // Estimate lines needed: success length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((success.len()
                        + format!("{success_icon} ", success_icon = self.icons.success).len())
                        as u16)
                        .div_ceil(available_width)
                        .saturating_add(2) // Include the top and bottom borders.
                        .clamp(3, 8)
                } else {
                    3
                }
            } else {
                3
            };
            constraints.push(Constraint::Length(message_height)); // Message area
        }

        constraints.push(Constraint::Length(footer_height)); // Footer for instructions

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Main content area - split horizontally if preview is shown
        let main_area = chunks[0];
        let (file_list_area, preview_area) = if self.ui.show_preview {
            let horizontal_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40), // File list takes 40%
                    Constraint::Percentage(60), // Preview takes 60%
                ])
                .split(main_area);
            (horizontal_chunks[0], Some(horizontal_chunks[1]))
        } else {
            (main_area, None)
        };

        // Main block with file list
        let file_items: Vec<ListItem> = if browsing.files().is_empty() {
            let has_query = self.file_search_query().is_some_and(|q| !q.is_empty());
            if self.is_searching_files() && has_query {
                vec![ListItem::new(format!(
                    "{search} No results found",
                    search = self.icons.search
                ))]
            } else {
                vec![ListItem::new(format!(
                    "{empty} No items found",
                    empty = self.icons.empty
                ))]
            }
        } else {
            browsing
                .files()
                .iter()
                .map(|file| ListItem::new(file.as_str()))
                .collect()
        };

        let mut list_state = ListState::default();
        if !browsing.files().is_empty() {
            list_state.select(Some(browsing.selected_index));
        }

        let current_path_display = if browsing.current_path().is_empty() {
            "/ (root)".to_string()
        } else {
            format!(
                "/{path}",
                path = browsing.current_path().trim_end_matches('/')
            )
        };

        let container_name =
            if let Some(container) = self.containers.get(self.selected_container_index) {
                &container.name
            } else {
                "Unknown"
            };

        let title = if self.is_searching_files() {
            format!(
                " {container} {path} · Search ({count} shown) ",
                container = container_name,
                path = current_path_display,
                count = browsing.files().len()
            )
        } else {
            format!(
                " {container} {path} ({count} items) ",
                container = container_name,
                path = current_path_display,
                count = browsing.files().len()
            )
        };

        let main_block = List::new(file_items)
            .block(
                panel()
                    .title(title)
                    .title_alignment(Alignment::Left)
                    .border_type(BorderType::Rounded),
            )
            .fg(TEXT)
            .highlight_style(selection())
            .highlight_symbol("▸ ");

        ratatui::widgets::StatefulWidget::render(main_block, file_list_area, buf, &mut list_state);

        // Render preview panel if active
        if let Some(preview_rect) = preview_area {
            self.render_preview_panel(preview_rect, buf);
        }

        let mut chunk_index = 1;

        // Search input if in search mode
        if self.is_searching_files() {
            let query = self.file_search_query().unwrap_or("");
            let search_text = format!("Search: {query}");
            let search_widget = Paragraph::new(search_text)
                .block(
                    panel()
                        .title(" Filter files ")
                        .border_type(BorderType::Rounded),
                )
                .fg(Color::Cyan)
                .alignment(Alignment::Left);
            search_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        // Error/Success/Loading message if present
        if let Some(error) = &self.error_message {
            let error_widget = Paragraph::new(format!(
                "{error_icon} {error}",
                error_icon = self.icons.error
            ))
            .block(panel().border_type(BorderType::Rounded))
            .fg(Color::Red)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .alignment(Alignment::Left);
            error_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        } else if let Some(success) = &self.success_message {
            let success_widget = Paragraph::new(format!(
                "{success_icon} {success}",
                success_icon = self.icons.success
            ))
            .block(panel().border_type(BorderType::Rounded))
            .fg(Color::Green)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .alignment(Alignment::Left);
            success_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        command_bar.render(chunks[chunk_index], buf);
    }

    /// Render the preview panel for CSV, TSV, or JSON files.
    fn render_preview_panel(&self, area: Rect, buf: &mut Buffer) {
        // Get the file type name for the title
        let file_type_name = self.preview_file_type.as_ref().map_or_else(
            || "Preview".to_string(),
            super::preview::PreviewFileType::display_name,
        );
        let preview_title_name =
            if self.preview_file_type == Some(super::preview::PreviewFileType::Parquet) {
                match self.parquet_preview_mode {
                    Some(ParquetPreviewMode::Table) => "Parquet Table".to_string(),
                    Some(ParquetPreviewMode::Metadata) => "Parquet Metadata".to_string(),
                    None => file_type_name.clone(),
                }
            } else {
                file_type_name.clone()
            };

        // Handle loading state; an open preview takes every key, so only its
        // own load can be running
        if self.tasks.is_running() {
            let loading = Paragraph::new(format!("{} Loading preview...", self.icons.loading))
                .block(
                    panel()
                        .title(format!(" {file_type_name} Preview "))
                        .title_alignment(Alignment::Left)
                        .border_type(BorderType::Rounded),
                )
                .fg(Color::Yellow)
                .alignment(Alignment::Center);
            loading.render(area, buf);
            return;
        }

        // Handle error state
        if let Some(error) = &self.preview_error {
            let error_widget = Paragraph::new(format!("{} {}", self.icons.error, error))
                .block(
                    panel()
                        .title(format!(" {file_type_name} Preview "))
                        .title_alignment(Alignment::Left)
                        .border_type(BorderType::Rounded),
                )
                .fg(Color::Red)
                .wrap(Wrap { trim: true })
                .alignment(Alignment::Center);
            error_widget.render(area, buf);
            return;
        }

        // Render based on preview data type
        match &self.preview_data {
            Some(PreviewData::Table(table)) => {
                self.render_table_preview(area, buf, table, &preview_title_name);
            }
            Some(PreviewData::Json(json)) => {
                self.render_json_preview(area, buf, json);
            }
            Some(PreviewData::Text(text)) => {
                self.render_text_preview(area, buf, text);
            }
            Some(PreviewData::ParquetSchema(schema)) => {
                self.render_parquet_schema_preview(area, buf, schema);
            }
            None => {
                let empty = Paragraph::new("No preview data available")
                    .block(
                        panel()
                            .title(format!(" {file_type_name} Preview "))
                            .title_alignment(Alignment::Left)
                            .border_type(BorderType::Rounded),
                    )
                    .fg(Color::DarkGray)
                    .alignment(Alignment::Center);
                empty.render(area, buf);
            }
        }
    }

    /// Render a table preview (for CSV, TSV, or JSON array of objects).
    #[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
    fn render_table_preview(
        &self,
        area: Rect,
        buf: &mut Buffer,
        table_data: &crate::preview::TablePreview,
        file_type_name: &str,
    ) {
        // Calculate column widths based on content
        let num_cols = table_data
            .headers
            .len()
            .max(table_data.rows.first().map_or(0, Vec::len));

        let row_info = if table_data.truncated {
            format!(
                "{} Preview ({}/{} rows, truncated)",
                file_type_name,
                table_data.rows.len(),
                table_data.total_rows
            )
        } else {
            format!(
                "{} Preview ({} rows)",
                file_type_name,
                table_data.rows.len()
            )
        };

        if num_cols == 0 {
            let empty = Paragraph::new("Empty table")
                .block(
                    panel()
                        .title(format!(" {row_info} "))
                        .title_alignment(Alignment::Left)
                        .border_type(BorderType::Rounded),
                )
                .fg(Color::DarkGray)
                .alignment(Alignment::Center);
            empty.render(area, buf);
            return;
        }

        // Calculate max width for each column (considering headers and data)
        let mut col_widths: Vec<usize> = vec![0; num_cols];

        // Consider header widths
        for (i, header) in table_data.headers.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(header.len());
            }
        }

        // Consider column type widths (for parquet table preview).
        if let Some(column_types) = &table_data.column_types {
            for (i, col_type) in column_types.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(col_type.len());
                }
            }
        }

        // Consider data widths (sample first 50 rows for efficiency)
        for row in table_data.rows.iter().take(50) {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Apply horizontal scroll offset and fit visible columns to viewport width.
        let col_offset = self.preview_scroll.1;
        let inner_width = area.width.saturating_sub(2) as usize;
        let viewport = compute_table_column_viewport(&col_widths, col_offset, inner_width);

        // Build constraints for visible columns
        let constraints: Vec<Constraint> = viewport
            .visible_indices
            .iter()
            .map(|idx| {
                let width = col_widths.get(*idx).copied().unwrap_or(3).max(3);
                Constraint::Length((width as u16) + 2)
            })
            .collect();

        let (start_col, end_col) = match (
            viewport.visible_indices.first(),
            viewport.visible_indices.last(),
        ) {
            (Some(start), Some(end)) => (start + 1, end + 1),
            _ => (1, 1),
        };
        let mut overflow_hints: Vec<&str> = Vec::new();
        if viewport.has_left_overflow {
            overflow_hints.push("← more");
        }
        if viewport.has_right_overflow {
            overflow_hints.push("→ more");
        }
        let overflow_text = if overflow_hints.is_empty() {
            String::new()
        } else {
            format!(" {}", overflow_hints.join(" | "))
        };
        let title = format!(" {row_info} • Cols {start_col}-{end_col}/{num_cols}{overflow_text} ");

        // Build header row
        let header_cells: Vec<Cell> = viewport
            .visible_indices
            .iter()
            .map(|idx| {
                Cell::from(table_data.headers.get(*idx).cloned().unwrap_or_default()).style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect();

        let header_row = Row::new(header_cells).height(1);

        let type_row = table_data.column_types.as_ref().map(|column_types| {
            let type_cells: Vec<Cell> = viewport
                .visible_indices
                .iter()
                .map(|idx| Cell::from(column_types.get(*idx).cloned().unwrap_or_default()))
                .collect();

            Row::new(type_cells).style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )
        });

        // Build data rows with scroll offset
        let data_rows: Vec<Row> = table_data
            .rows
            .iter()
            .enumerate()
            .map(|(row_idx, row)| {
                let cells: Vec<Cell> = viewport
                    .visible_indices
                    .iter()
                    .map(|idx| Cell::from(row.get(*idx).cloned().unwrap_or_default()))
                    .collect();

                let style = if row_idx == self.preview_selected_row {
                    selection()
                } else {
                    Style::default().fg(Color::White)
                };

                Row::new(cells).style(style).height(1)
            })
            .collect();

        let mut rows = Vec::with_capacity(data_rows.len() + usize::from(type_row.is_some()));
        if let Some(type_row) = type_row {
            rows.push(type_row);
        }
        rows.extend(data_rows);

        // Create the table widget
        let table = Table::new(rows, constraints)
            .header(header_row)
            .block(
                panel()
                    .title(title)
                    .title_alignment(Alignment::Left)
                    .border_type(BorderType::Rounded),
            )
            .row_highlight_style(selection());

        // Use TableState for scrolling
        let mut table_state = TableState::default();
        let selected_row =
            self.preview_selected_row + usize::from(table_data.column_types.is_some());
        table_state.select(Some(selected_row));

        ratatui::widgets::StatefulWidget::render(table, area, buf, &mut table_state);
    }

    /// Render a JSON preview (for non-tabular JSON).
    fn render_json_preview(
        &self,
        area: Rect,
        buf: &mut Buffer,
        json_data: &crate::preview::JsonPreview,
    ) {
        // Build title based on mode and truncation state
        let title = if json_data.is_raw {
            " JSON Preview (raw, truncated at 50KB) ".to_string()
        } else if json_data.truncated {
            format!(
                " JSON Preview ({}/{} lines, truncated) ",
                json_data.content.lines().count(),
                json_data.total_lines
            )
        } else {
            format!(" JSON Preview ({} lines) ", json_data.total_lines)
        };

        // Apply vertical scroll offset
        let mut visible_lines: Vec<&str> = json_data
            .content
            .lines()
            .skip(self.preview_selected_row)
            .collect();

        // Add truncation indicator at the bottom if truncated
        let truncation_indicator = if json_data.truncated {
            Some("... [truncated at 50KB]")
        } else {
            None
        };

        // Build the visible content
        let visible_content = if let Some(indicator) = truncation_indicator {
            // Only show indicator if we're near the bottom
            let content_height = area.height.saturating_sub(2) as usize; // Account for borders
            if visible_lines.len() <= content_height {
                visible_lines.push("");
                visible_lines.push(indicator);
            }
            visible_lines.join("\n")
        } else {
            visible_lines.join("\n")
        };

        let json_widget = Paragraph::new(visible_content)
            .block(
                panel()
                    .title(title)
                    .title_alignment(Alignment::Left)
                    .border_type(BorderType::Rounded),
            )
            .fg(Color::White)
            .wrap(Wrap { trim: false });

        json_widget.render(area, buf);
    }

    /// Render a text file preview.
    fn render_text_preview(
        &self,
        area: Rect,
        buf: &mut Buffer,
        text_data: &crate::preview::TextPreview,
    ) {
        // Build title with extension and line count
        let title = if text_data.truncated {
            format!(
                " {} Preview ({} lines, truncated at 50KB) ",
                text_data.extension, text_data.total_lines
            )
        } else {
            format!(
                " {} Preview ({} lines) ",
                text_data.extension, text_data.total_lines
            )
        };

        // Apply vertical scroll offset
        let mut visible_lines: Vec<&str> = text_data
            .content
            .lines()
            .skip(self.preview_selected_row)
            .collect();

        // Add truncation indicator at the bottom if truncated
        let truncation_indicator = if text_data.truncated {
            Some("... [truncated at 50KB]")
        } else {
            None
        };

        // Build the visible content
        let visible_content = if let Some(indicator) = truncation_indicator {
            // Only show indicator if we're near the bottom
            let content_height = area.height.saturating_sub(2) as usize; // Account for borders
            if visible_lines.len() <= content_height {
                visible_lines.push("");
                visible_lines.push(indicator);
            }
            visible_lines.join("\n")
        } else {
            visible_lines.join("\n")
        };

        let text_widget = Paragraph::new(visible_content)
            .block(
                panel()
                    .title(title)
                    .title_alignment(Alignment::Left)
                    .border_type(BorderType::Rounded),
            )
            .fg(Color::White)
            .wrap(Wrap { trim: false });

        text_widget.render(area, buf);
    }

    /// Render a Parquet schema preview.
    #[allow(clippy::vec_init_then_push)]
    fn render_parquet_schema_preview(
        &self,
        area: Rect,
        buf: &mut Buffer,
        schema_data: &crate::preview::ParquetSchemaPreview,
    ) {
        let title = format!(" Parquet Metadata ({} columns) ", schema_data.fields.len());

        // Build metadata section
        let mut lines: Vec<Line> = Vec::new();

        // File metadata header
        lines.push(Line::from(vec![
            Span::styled(
                "── Metadata ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(30), Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(""));

        if let Some(note) = &schema_data.note {
            lines.push(Line::from(vec![
                Span::styled("  Note: ", Style::default().fg(Color::Yellow)),
                Span::styled(note.clone(), Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(""));
        }

        // Row count
        lines.push(Line::from(vec![
            Span::styled("  Rows:        ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format_number(schema_data.num_rows.cast_unsigned()),
                Style::default().fg(Color::White),
            ),
        ]));

        // Row groups
        lines.push(Line::from(vec![
            Span::styled("  Row Groups:  ", Style::default().fg(Color::Yellow)),
            Span::styled(
                schema_data.num_row_groups.to_string(),
                Style::default().fg(Color::White),
            ),
        ]));

        // File size (if known)
        if let Some(size) = schema_data.file_size {
            lines.push(Line::from(vec![
                Span::styled("  File Size:   ", Style::default().fg(Color::Yellow)),
                Span::styled(format_bytes(size), Style::default().fg(Color::White)),
            ]));
        }

        // Created by (if known)
        if let Some(ref created_by) = schema_data.created_by {
            lines.push(Line::from(vec![
                Span::styled("  Created By:  ", Style::default().fg(Color::Yellow)),
                Span::styled(created_by.clone(), Style::default().fg(Color::White)),
            ]));
        }

        lines.push(Line::from(""));

        // Schema header
        lines.push(Line::from(vec![
            Span::styled(
                "── Schema ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(31), Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(""));

        // Schema fields
        for field in &schema_data.fields {
            // Parse the field into name and type
            if let Some((name, type_str)) = field.split_once(": ") {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(name.to_string(), Style::default().fg(Color::Green)),
                    Span::styled(": ", Style::default().fg(Color::DarkGray)),
                    Span::styled(type_str.to_string(), Style::default().fg(Color::Magenta)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(field.clone(), Style::default().fg(Color::White)),
                ]));
            }
        }

        // Apply scroll offset
        let visible_lines: Vec<Line> = lines.into_iter().skip(self.preview_selected_row).collect();

        let text = Text::from(visible_lines);

        let schema_widget = Paragraph::new(text).block(
            panel()
                .title(title)
                .title_alignment(Alignment::Left)
                .border_type(BorderType::Rounded),
        );

        schema_widget.render(area, buf);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableColumnViewport {
    visible_indices: Vec<usize>,
    has_left_overflow: bool,
    has_right_overflow: bool,
}

/// Compute which table columns fit within the current viewport width.
fn compute_table_column_viewport(
    col_widths: &[usize],
    col_offset: usize,
    inner_width: usize,
) -> TableColumnViewport {
    if col_widths.is_empty() {
        return TableColumnViewport {
            visible_indices: Vec::new(),
            has_left_overflow: false,
            has_right_overflow: false,
        };
    }

    let start = col_offset.min(col_widths.len().saturating_sub(1));
    let mut visible_indices = Vec::new();
    let mut used_width = 0usize;

    for (idx, width) in col_widths.iter().enumerate().skip(start) {
        let col_width = (*width).max(3) + 2; // +2 for cell padding
        let exceeds = used_width + col_width > inner_width;

        if !visible_indices.is_empty() && exceeds {
            break;
        }

        visible_indices.push(idx);
        used_width += col_width;

        // Always render at least one column, even if it exceeds the viewport width.
        if visible_indices.len() == 1 && exceeds {
            break;
        }
    }

    if visible_indices.is_empty() {
        visible_indices.push(start);
    }

    let has_left_overflow = start > 0;
    let has_right_overflow = visible_indices
        .last()
        .is_some_and(|last| *last + 1 < col_widths.len());

    TableColumnViewport {
        visible_indices,
        has_left_overflow,
        has_right_overflow,
    }
}

/// Format bytes in human-readable format
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: f64 = 1024.0;

    if bytes == 0 {
        return "0 B".to_string();
    }

    let bytes_f = bytes as f64;
    let unit_index = (bytes_f.log10() / THRESHOLD.log10()).floor() as usize;
    let unit_index = unit_index.min(UNITS.len() - 1);

    let size = bytes_f / THRESHOLD.powi(unit_index as i32);

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

fn truncate_with_ellipsis(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    if max_chars <= 3 {
        return "...".to_string();
    }
    let prefix: String = input.chars().take(max_chars - 3).collect();
    format!("{prefix}...")
}

/// Format a number with thousand separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::{compute_table_column_viewport, truncate_with_ellipsis};
    use proptest::prelude::*;

    #[test]
    fn viewport_marks_left_and_right_overflow() {
        let widths = vec![6, 6, 6, 6, 6];
        // Each rendered column takes width+2 = 8 chars, so only two fit into 20 chars.
        let viewport = compute_table_column_viewport(&widths, 1, 20);
        assert_eq!(viewport.visible_indices, vec![1, 2]);
        assert!(viewport.has_left_overflow);
        assert!(viewport.has_right_overflow);
    }

    #[test]
    fn viewport_shows_single_column_when_too_narrow() {
        let widths = vec![40, 10, 10];
        let viewport = compute_table_column_viewport(&widths, 0, 5);
        assert_eq!(viewport.visible_indices, vec![0]);
        assert!(!viewport.has_left_overflow);
        assert!(viewport.has_right_overflow);
    }

    #[test]
    fn viewport_clamps_offset_past_end() {
        let widths = vec![4, 4, 4];
        let viewport = compute_table_column_viewport(&widths, 99, 20);
        assert_eq!(viewport.visible_indices, vec![2]);
        assert!(viewport.has_left_overflow);
        assert!(!viewport.has_right_overflow);
    }

    proptest! {
        #[test]
        fn ellipsis_truncation_respects_requested_limits(input in ".*", max_chars in 0_usize..32) {
            let truncated = truncate_with_ellipsis(&input, max_chars);
            if input.chars().count() <= max_chars {
                prop_assert_eq!(truncated, input);
            } else if max_chars <= 3 {
                prop_assert_eq!(truncated, "...");
            } else {
                prop_assert!(truncated.chars().count() <= max_chars);
                prop_assert!(truncated.ends_with("..."));
            }
        }
    }
}
