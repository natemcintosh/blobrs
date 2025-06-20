use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Widget, Wrap},
};

use crate::app::{App, AppState};

impl Widget for &App {
    /// Renders the user interface widgets.
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.state {
            AppState::ContainerSelection => self.render_container_selection(area, buf),
            AppState::BlobBrowsing => {
                self.render_blob_browsing(area, buf);

                // Render popup over the blob browsing view if needed
                if self.show_blob_info_popup {
                    self.render_blob_info_popup(area, buf);
                } else if self.show_download_picker {
                    self.render_download_picker_popup(area, buf);
                } else if self.is_downloading {
                    self.render_download_progress_popup(area, buf);
                }
            }
        }
    }
}

impl App {
    /// Calculate the height needed for footer text with wrapping
    fn calculate_footer_height(&self, text: &str, available_width: u16) -> u16 {
        if available_width <= 4 {
            return 3; // Minimum height for borders and padding
        }

        // Account for borders and padding (2 for left/right borders, 2 for padding)
        let text_width = available_width.saturating_sub(4);

        if text_width == 0 {
            return 3;
        }

        // Calculate lines needed: text length / available width, with minimum of 3 and maximum of 6
        let lines_needed = (text.len() as u16).div_ceil(text_width).clamp(1, 4);

        // Add 2 for top and bottom borders
        lines_needed + 2
    }

    fn render_container_selection(&self, area: Rect, buf: &mut Buffer) {
        // Calculate footer height based on instruction text
        let instructions = if self.container_search_mode {
            "Search Mode: Type to filter containers • `Enter` to confirm • `Esc` to cancel • `Ctrl+↑`/`Ctrl+↓` to navigate"
        } else {
            "Press `Ctrl-C` or `q` or `Esc` to quit • `r`/`F5` to refresh • `↑`/`↓` or `k`/`j` to navigate • `→`/`l`/`Enter` to select container • `/` to search"
        };
        let footer_height = self.calculate_footer_height(instructions, area.width);

        // Create a vertical layout
        let mut constraints = vec![
            Constraint::Min(0), // Main content area
        ];

        // Add space for search input if in container search mode
        if self.container_search_mode {
            constraints.push(Constraint::Length(3)); // Search input area
        }

        // Add space for error message if present
        if self.error_message.is_some() || self.is_loading {
            // Calculate height based on error message length and terminal width
            let error_height = if let Some(error) = &self.error_message {
                // Estimate lines needed: error length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((error.len() + format!("{} ", self.icons.error).len()) as u16)
                        .div_ceil(available_width)
                        .clamp(3, 8)
                } else {
                    3
                }
            } else {
                3
            };
            constraints.push(Constraint::Length(error_height)); // Error/loading area
        }

        constraints.push(Constraint::Length(footer_height)); // Footer for instructions

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Container list
        let container_items: Vec<ListItem> = if self.is_loading {
            vec![ListItem::new(format!(
                "{} Loading containers...",
                self.icons.loading
            ))]
        } else if self.containers.is_empty() {
            if self.container_search_mode && !self.container_search_query.is_empty() {
                vec![ListItem::new(format!(
                    "{} No containers found matching search",
                    self.icons.search
                ))]
            } else {
                vec![ListItem::new(format!(
                    "{} No containers found",
                    self.icons.empty
                ))]
            }
        } else {
            self.containers
                .iter()
                .map(|container| ListItem::new(format!("{} {}", self.icons.folder, container.name)))
                .collect()
        };

        let mut list_state = ListState::default();
        if !self.is_loading && !self.containers.is_empty() {
            list_state.select(Some(self.selected_container_index));
        }

        let title = if self.container_search_mode {
            format!(
                " Azure Storage Account: {} - Select Container [SEARCH] ({} shown) ",
                self.storage_account,
                self.containers.len()
            )
        } else {
            format!(
                " Azure Storage Account: {} - Select Container ({} containers) ",
                self.storage_account,
                self.containers.len()
            )
        };

        let main_block = List::new(container_items)
            .block(
                Block::bordered()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .fg(Color::Green)
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow))
            .highlight_symbol("▶ ");

        ratatui::widgets::StatefulWidget::render(main_block, chunks[0], buf, &mut list_state);

        let mut chunk_index = 1;

        // Search input if in container search mode
        if self.container_search_mode {
            let search_text = format!("Search: {}", self.container_search_query);
            let search_widget = Paragraph::new(search_text)
                .block(
                    Block::bordered()
                        .title(" Search Containers (Press Enter to confirm, Esc to cancel) ")
                        .border_type(BorderType::Rounded),
                )
                .fg(Color::Cyan)
                .alignment(Alignment::Left);
            search_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        // Error/Loading message if present
        if let Some(error) = &self.error_message {
            let error_widget = Paragraph::new(format!("{} {}", self.icons.error, error))
                .block(Block::bordered().border_type(BorderType::Rounded))
                .fg(Color::Red)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .alignment(Alignment::Left);
            error_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        } else if self.is_loading {
            let loading_widget =
                Paragraph::new(format!("{} Loading containers...", self.icons.loading))
                    .block(Block::bordered().border_type(BorderType::Rounded))
                    .fg(Color::Yellow)
                    .alignment(Alignment::Center);
            loading_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        // Footer with instructions (using pre-calculated text)
        let footer = Paragraph::new(instructions)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .fg(Color::Cyan)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        footer.render(chunks[chunk_index], buf);
    }

    fn render_blob_browsing(&self, area: Rect, buf: &mut Buffer) {
        // Calculate footer height based on instruction text
        let instructions = if self.search_mode {
            "Search Mode: Type to filter • `Enter` to confirm • `Esc` to cancel • `Ctrl+↑`/`Ctrl+↓` to navigate"
        } else {
            "Press `Ctrl-C` or `q` to quit • `Esc`/`←`/`h` to go back • `r`/`F5` to refresh • `↑`/`↓` or `k`/`j` to navigate • `→`/`l`/`Enter` to enter folder • `/` to search • `i` for info • `d` to download • `Backspace` for container selection"
        };
        let footer_height = self.calculate_footer_height(instructions, area.width);

        // Create a vertical layout with main content, search (if active), error/loading, and footer
        let mut constraints = vec![
            Constraint::Min(0), // Main content area
        ];

        // Add space for search input if in search mode
        if self.search_mode {
            constraints.push(Constraint::Length(3)); // Search input area
        }

        // Add space for error message if present
        if self.error_message.is_some() || self.is_loading {
            // Calculate height based on error message length and terminal width
            let error_height = if let Some(error) = &self.error_message {
                // Estimate lines needed: error length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((error.len() + format!("{} ", self.icons.error).len()) as u16)
                        .div_ceil(available_width)
                        .clamp(3, 8)
                } else {
                    3
                }
            } else {
                3
            };
            constraints.push(Constraint::Length(error_height)); // Error/loading area
        }

        constraints.push(Constraint::Length(footer_height)); // Footer for instructions

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Main block with file list
        let file_items: Vec<ListItem> = if self.is_loading {
            vec![ListItem::new(format!("{} Loading...", self.icons.loading))]
        } else if self.files.is_empty() {
            if self.search_mode && !self.search_query.is_empty() {
                vec![ListItem::new(format!(
                    "{} No results found",
                    self.icons.search
                ))]
            } else {
                vec![ListItem::new(format!(
                    "{} No items found",
                    self.icons.empty
                ))]
            }
        } else {
            self.files
                .iter()
                .map(|file| ListItem::new(file.as_str()))
                .collect()
        };

        let mut list_state = ListState::default();
        if !self.is_loading && !self.files.is_empty() {
            list_state.select(Some(self.selected_index));
        }

        let current_path_display = if self.current_path.is_empty() {
            "/ (root)".to_string()
        } else {
            format!("/{}", self.current_path.trim_end_matches('/'))
        };

        let container_name =
            if let Some(container) = self.containers.get(self.selected_container_index) {
                &container.name
            } else {
                "Unknown"
            };

        let title = if self.search_mode {
            format!(
                " Container: {} - {} [SEARCH] ({} shown) ",
                container_name,
                current_path_display,
                self.files.len()
            )
        } else {
            format!(
                " Container: {} - {} ({} items) ",
                container_name,
                current_path_display,
                self.files.len()
            )
        };

        let main_block = List::new(file_items)
            .block(
                Block::bordered()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .fg(Color::Green)
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow))
            .highlight_symbol("▶ ");

        ratatui::widgets::StatefulWidget::render(main_block, chunks[0], buf, &mut list_state);

        let mut chunk_index = 1;

        // Search input if in search mode
        if self.search_mode {
            let search_text = format!("Search: {}", self.search_query);
            let search_widget = Paragraph::new(search_text)
                .block(
                    Block::bordered()
                        .title(" Search (Press Enter to confirm, Esc to cancel) ")
                        .border_type(BorderType::Rounded),
                )
                .fg(Color::Cyan)
                .alignment(Alignment::Left);
            search_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        // Error/Loading message if present
        if let Some(error) = &self.error_message {
            let error_widget = Paragraph::new(format!("{} {}", self.icons.error, error))
                .block(Block::bordered().border_type(BorderType::Rounded))
                .fg(Color::Red)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .alignment(Alignment::Left);
            error_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        } else if self.is_loading {
            let loading_widget = Paragraph::new(format!(
                "{} Loading Azure Blob Storage...",
                self.icons.loading
            ))
            .block(Block::bordered().border_type(BorderType::Rounded))
            .fg(Color::Yellow)
            .alignment(Alignment::Center);
            loading_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        // Footer with instructions (using pre-calculated text)
        let footer = Paragraph::new(instructions)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .fg(Color::Cyan)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        footer.render(chunks[chunk_index], buf);
    }

    fn render_blob_info_popup(&self, area: Rect, buf: &mut Buffer) {
        // Calculate popup size and position
        let popup_width = area.width.clamp(40, 60); // Between 40 and 60 characters wide
        let popup_height = area.height.clamp(10, 20); // Between 10 and 20 lines tall

        // Center the popup
        let popup_x = (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: area.x + popup_x,
            y: area.y + popup_y,
            width: popup_width,
            height: popup_height,
        };

        // Clear the popup area (make it semi-transparent effect by using a background)
        let clear_block = Block::bordered()
            .border_type(BorderType::Rounded)
            .style(Style::default().bg(Color::Black));
        clear_block.render(popup_area, buf);

        if let Some(ref blob_info) = self.current_blob_info {
            // Create layout for popup content
            let inner_area = Rect {
                x: popup_area.x + 1,
                y: popup_area.y + 1,
                width: popup_area.width.saturating_sub(2),
                height: popup_area.height.saturating_sub(2),
            };

            let _chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),    // Main info area
                    Constraint::Length(1), // Footer
                ])
                .split(inner_area);

            let mut info_lines = Vec::new();

            // Title
            if blob_info.is_folder {
                info_lines.push(format!("{} Folder Information", self.icons.folder));
            } else {
                info_lines.push(format!("{} Blob Information", self.icons.file));
            }
            info_lines.push(String::new()); // Empty line

            // Name
            let name_display = if blob_info.name.len() > (popup_width as usize).saturating_sub(8) {
                format!(
                    "{}...",
                    &blob_info.name[0..(popup_width as usize).saturating_sub(11)]
                )
            } else {
                blob_info.name.clone()
            };
            info_lines.push(format!("Name: {}", name_display));
            info_lines.push(String::new()); // Empty line

            if blob_info.is_folder {
                // Folder-specific information
                if let Some(blob_count) = blob_info.blob_count {
                    info_lines.push(format!("Blobs: {}", blob_count));
                }
                if let Some(total_size) = blob_info.total_size {
                    info_lines.push(format!("Total size: {}", format_bytes(total_size)));
                }
            } else {
                // Blob-specific information
                if let Some(size) = blob_info.size {
                    info_lines.push(format!("Size: {}", format_bytes(size)));
                }
                if let Some(ref last_modified) = blob_info.last_modified {
                    info_lines.push(format!("Modified: {}", last_modified));
                }
                if let Some(ref etag) = blob_info.etag {
                    let etag_display = if etag.len() > (popup_width as usize).saturating_sub(8) {
                        format!("{}...", &etag[0..(popup_width as usize).saturating_sub(11)])
                    } else {
                        etag.clone()
                    };
                    info_lines.push(format!("ETag: {}", etag_display));
                }
            }

            let info_text = info_lines.join("\n");
            let info_paragraph = Paragraph::new(info_text)
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(" Information ")
                        .style(Style::default().fg(Color::Cyan).bg(Color::Black)),
                )
                .style(Style::default().bg(Color::Black))
                .wrap(ratatui::widgets::Wrap { trim: true });

            info_paragraph.render(popup_area, buf);

            // Footer with instructions (overlaid at bottom of popup)
            let footer_area = Rect {
                x: popup_area.x + 2,
                y: popup_area.y + popup_area.height.saturating_sub(2),
                width: popup_area.width.saturating_sub(4),
                height: 1,
            };

            let instructions = "Press Esc, ← or h to close";
            let footer_text = Paragraph::new(instructions)
                .style(Style::default().fg(Color::Yellow).bg(Color::Black))
                .alignment(Alignment::Center);

            footer_text.render(footer_area, buf);
        }
    }

    /// Render the download destination picker popup.
    fn render_download_picker_popup(&self, area: Rect, buf: &mut Buffer) {
        // Calculate popup size
        let popup_width = (area.width * 3 / 4).min(60);
        let popup_height = 8;

        // Center the popup
        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        // Clear the popup area with a background
        for y in popup_area.y..popup_area.y + popup_area.height {
            for x in popup_area.x..popup_area.x + popup_area.width {
                buf[(x, y)].set_style(Style::default().bg(Color::Black));
            }
        }

        let selected_file = if !self.files.is_empty() {
            &self.files[self.selected_index]
        } else {
            "No file selected"
        };

        // Extract the name without the icon prefix
        let folder_prefix = format!("{} ", self.icons.folder);
        let file_prefix = format!("{} ", self.icons.file);
        let name = if selected_file.starts_with(&folder_prefix) {
            selected_file
                .strip_prefix(&folder_prefix)
                .unwrap_or(selected_file)
        } else if selected_file.starts_with(&file_prefix) {
            selected_file
                .strip_prefix(&file_prefix)
                .unwrap_or(selected_file)
        } else {
            selected_file
        };

        let download_text = [
            format!("Ready to download: {}", name),
            String::new(),
            "Press Enter to select download destination".to_string(),
            "Press Esc to cancel".to_string(),
        ];

        let info_text = download_text.join("\n");
        let info_paragraph = Paragraph::new(info_text)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(" Download ")
                    .style(Style::default().fg(Color::Green).bg(Color::Black)),
            )
            .style(Style::default().bg(Color::Black))
            .alignment(Alignment::Center);

        info_paragraph.render(popup_area, buf);
    }

    /// Render the download progress popup.
    fn render_download_progress_popup(&self, area: Rect, buf: &mut Buffer) {
        if let Some(progress) = &self.download_progress {
            // Calculate popup size
            let popup_width = (area.width * 3 / 4).min(70);
            let popup_height = 12;

            // Center the popup
            let popup_area = Rect {
                x: (area.width.saturating_sub(popup_width)) / 2,
                y: (area.height.saturating_sub(popup_height)) / 2,
                width: popup_width,
                height: popup_height,
            };

            // Clear the popup area with a background
            for y in popup_area.y..popup_area.y + popup_area.height {
                for x in popup_area.x..popup_area.x + popup_area.width {
                    buf[(x, y)].set_style(Style::default().bg(Color::Black));
                }
            }

            let mut progress_lines = vec![
                format!("Downloading: {}", progress.current_file),
                String::new(),
                format!(
                    "Files: {} / {}",
                    progress.files_completed, progress.total_files
                ),
            ];

            // Add bytes downloaded if available
            if let Some(total_bytes) = progress.total_bytes {
                let percentage = if total_bytes > 0 {
                    (progress.bytes_downloaded as f64 / total_bytes as f64 * 100.0) as u8
                } else {
                    100
                };
                progress_lines.push(format!(
                    "Size: {} / {} ({}%)",
                    format_bytes(progress.bytes_downloaded),
                    format_bytes(total_bytes),
                    percentage
                ));
            } else {
                progress_lines.push(format!(
                    "Downloaded: {}",
                    format_bytes(progress.bytes_downloaded)
                ));
            }

            // Add error message if present
            if let Some(error) = &progress.error_message {
                progress_lines.push(String::new());
                progress_lines.push(format!("Error: {}", error));
            }

            let info_text = progress_lines.join("\n");
            let info_paragraph = Paragraph::new(info_text)
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(" Download Progress ")
                        .style(Style::default().fg(Color::Yellow).bg(Color::Black)),
                )
                .style(Style::default().bg(Color::Black));

            info_paragraph.render(popup_area, buf);
        }
    }
}

/// Format bytes in human-readable format
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
