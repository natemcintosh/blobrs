use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    widgets::{
        Block, BorderType, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState,
        Widget, Wrap,
    },
};

use crate::app::{App, AppState};
use crate::preview::PreviewData;

impl Widget for &App {
    /// Renders the user interface widgets.
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.state {
            AppState::ContainerSelection => self.render_container_selection(area, buf),
            AppState::BlobBrowsing => {
                self.render_blob_browsing(area, buf);

                // Render popup over the blob browsing view if needed
                if self.show_delete_dialog {
                    self.render_delete_dialog_popup(area, buf);
                } else if self.is_deleting {
                    self.render_delete_progress_popup(area, buf);
                } else if self.show_clone_dialog {
                    self.render_clone_dialog_popup(area, buf);
                } else if self.is_cloning {
                    self.render_clone_progress_popup(area, buf);
                } else if self.show_blob_info_popup {
                    self.render_blob_info_popup(area, buf);
                } else if self.show_download_picker {
                    self.render_download_picker_popup(area, buf);
                } else if self.show_sort_popup {
                    App::render_sort_popup(area, buf);
                } else if self.is_downloading {
                    self.render_download_progress_popup(area, buf);
                }
            }
        }
    }
}

impl App {
    /// Calculate the height needed for footer text with wrapping
    #[allow(clippy::cast_possible_truncation)] // text length is always small for UI
    fn calculate_footer_height(text: &str, available_width: u16) -> u16 {
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
        let footer_height = Self::calculate_footer_height(instructions, area.width);

        // Create a vertical layout
        let mut constraints = vec![
            Constraint::Min(0), // Main content area
        ];

        // Add space for search input if in container search mode
        if self.container_search_mode {
            constraints.push(Constraint::Length(3)); // Search input area
        }

        // Add space for error or success message if present
        if self.error_message.is_some() || self.success_message.is_some() || self.is_loading {
            // Calculate height based on message length and terminal width
            #[allow(clippy::cast_possible_truncation)] // UI text lengths are always small
            let message_height = if let Some(error) = &self.error_message {
                // Estimate lines needed: error length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((error.len() + format!("{} ", self.icons.error).len()) as u16)
                        .div_ceil(available_width)
                        .clamp(3, 8)
                } else {
                    3
                }
            } else if let Some(success) = &self.success_message {
                // Estimate lines needed: success length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((success.len() + format!("{} ", self.icons.success).len()) as u16)
                        .div_ceil(available_width)
                        .clamp(3, 8)
                } else {
                    3
                }
            } else {
                3
            };
            constraints.push(Constraint::Length(message_height)); // Message/loading area
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

        // Error/Success/Loading message if present
        if let Some(error) = &self.error_message {
            let error_widget = Paragraph::new(format!("{} {}", self.icons.error, error))
                .block(Block::bordered().border_type(BorderType::Rounded))
                .fg(Color::Red)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .alignment(Alignment::Left);
            error_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        } else if let Some(success) = &self.success_message {
            let success_widget = Paragraph::new(format!("{} {}", self.icons.success, success))
                .block(Block::bordered().border_type(BorderType::Rounded))
                .fg(Color::Green)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .alignment(Alignment::Left);
            success_widget.render(chunks[chunk_index], buf);
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
        } else if self.show_preview {
            "Preview: `↑`/`↓`/`k`/`j` to scroll rows • `←`/`→`/`h`/`l` to scroll columns • `p` or `Esc` to close preview"
        } else {
            "Press `Ctrl-C` or `q` to quit • `Esc`/`←`/`h` to go back • `r`/`F5` to refresh • `↑`/`↓` or `k`/`j` to navigate • `→`/`l`/`Enter` to enter folder • `/` to search • `s` to sort • `i` for info • `p` to preview • `y` to copy path • `c` to clone • `x` to delete • `d` to download"
        };
        let footer_height = Self::calculate_footer_height(instructions, area.width);

        // Create a vertical layout with main content, search (if active), error/loading, and footer
        let mut constraints = vec![
            Constraint::Min(0), // Main content area
        ];

        // Add space for search input if in search mode
        if self.search_mode {
            constraints.push(Constraint::Length(3)); // Search input area
        }

        // Add space for error or success message if present
        if self.error_message.is_some() || self.success_message.is_some() || self.is_loading {
            // Calculate height based on message length and terminal width
            #[allow(clippy::cast_possible_truncation)] // UI text lengths are always small
            let message_height = if let Some(error) = &self.error_message {
                // Estimate lines needed: error length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((error.len() + format!("{} ", self.icons.error).len()) as u16)
                        .div_ceil(available_width)
                        .clamp(3, 8)
                } else {
                    3
                }
            } else if let Some(success) = &self.success_message {
                // Estimate lines needed: success length / (width - padding), min 3, max 8
                let available_width = area.width.saturating_sub(4); // Account for borders and padding
                if available_width > 0 {
                    ((success.len() + format!("{} ", self.icons.success).len()) as u16)
                        .div_ceil(available_width)
                        .clamp(3, 8)
                } else {
                    3
                }
            } else {
                3
            };
            constraints.push(Constraint::Length(message_height)); // Message/loading area
        }

        constraints.push(Constraint::Length(footer_height)); // Footer for instructions

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Main content area - split horizontally if preview is shown
        let main_area = chunks[0];
        let (file_list_area, preview_area) = if self.show_preview {
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

        ratatui::widgets::StatefulWidget::render(main_block, file_list_area, buf, &mut list_state);

        // Render preview panel if active
        if let Some(preview_rect) = preview_area {
            self.render_preview_panel(preview_rect, buf);
        }

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

        // Error/Success/Loading message if present
        if let Some(error) = &self.error_message {
            let error_widget = Paragraph::new(format!("{} {}", self.icons.error, error))
                .block(Block::bordered().border_type(BorderType::Rounded))
                .fg(Color::Red)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .alignment(Alignment::Left);
            error_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        } else if let Some(success) = &self.success_message {
            let success_widget = Paragraph::new(format!("{} {}", self.icons.success, success))
                .block(Block::bordered().border_type(BorderType::Rounded))
                .fg(Color::Green)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .alignment(Alignment::Left);
            success_widget.render(chunks[chunk_index], buf);
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
            info_lines.push(format!("Name: {name_display}"));
            info_lines.push(String::new()); // Empty line

            if blob_info.is_folder {
                // Folder-specific information
                if let Some(blob_count) = blob_info.blob_count {
                    info_lines.push(format!("Blobs: {blob_count}"));
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
                    info_lines.push(format!("Modified: {last_modified}"));
                }
                if let Some(ref etag) = blob_info.etag {
                    let etag_display = if etag.len() > (popup_width as usize).saturating_sub(8) {
                        format!("{}...", &etag[0..(popup_width as usize).saturating_sub(11)])
                    } else {
                        etag.clone()
                    };
                    info_lines.push(format!("ETag: {etag_display}"));
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

        let selected_file = if self.files.is_empty() {
            "No file selected"
        } else {
            &self.files[self.selected_index]
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
            format!("Ready to download: {name}"),
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
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
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
                progress_lines.push(format!("Error: {error}"));
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

    /// Render the sort selection popup.
    fn render_sort_popup(area: Rect, buf: &mut Buffer) {
        // Calculate popup size
        let popup_width = 50;
        let popup_height = 10;

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

        let sort_text = [
            "Select sorting criteria:".to_string(),
            String::new(),
            "n - Sort by Name".to_string(),
            "m - Sort by Date Modified".to_string(),
            "t - Sort by Date Created".to_string(),
            "s - Sort by Size".to_string(),
            String::new(),
            "Press Esc to cancel".to_string(),
        ];

        let info_text = sort_text.join("\n");
        let info_paragraph = Paragraph::new(info_text)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(" Sort Files ")
                    .style(Style::default().fg(Color::Cyan).bg(Color::Black)),
            )
            .style(Style::default().bg(Color::Black))
            .alignment(Alignment::Left);

        info_paragraph.render(popup_area, buf);
    }

    /// Render the clone dialog popup.
    fn render_clone_dialog_popup(&self, area: Rect, buf: &mut Buffer) {
        // Calculate popup size
        let popup_width = (area.width * 3 / 4).min(70);

        let item_type = if self.clone_is_folder {
            "folder"
        } else {
            "blob"
        };
        let can_confirm =
            self.clone_input != self.clone_original_path && !self.clone_input.is_empty();

        let enter_hint = if can_confirm {
            "Enter to confirm"
        } else {
            "Enter to confirm (change name first)"
        };

        // Calculate wrapped line counts for dynamic height
        // Account for borders (2 chars)
        let content_width = popup_width.saturating_sub(2) as usize;

        let original_line = format!("Original: {}", self.clone_original_path);

        // Calculate how many lines the original path will take when wrapped
        #[allow(clippy::cast_possible_truncation)]
        let original_lines = if content_width > 0 {
            original_line.len().div_ceil(content_width).max(1) as u16
        } else {
            1
        };

        // New path input is always 1 line (scrolling)
        // Calculate total height: title line (1) + blank (1) + original lines + blank (1) + new path (1) + blank (1) + hint line (1) + borders (2)
        let popup_height = (1 + 1 + original_lines + 1 + 1 + 1 + 1 + 2).max(10);

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

        // For the new path input, show a scrolling view that keeps cursor visible
        let new_path_prefix = "New path: ";
        let available_input_width = content_width.saturating_sub(new_path_prefix.len());

        // Calculate visible portion of input - scroll to keep cursor at end visible
        let visible_input = if self.clone_input.len() > available_input_width {
            let start = self.clone_input.len() - available_input_width;
            format!("…{}", &self.clone_input[start + 1..])
        } else {
            self.clone_input.clone()
        };

        let new_path_display = format!("{new_path_prefix}{visible_input}");

        let clone_text = [
            format!("Clone {item_type} to new path:"),
            String::new(),
            original_line,
            String::new(),
            new_path_display.clone(),
            String::new(),
            format!("{enter_hint} • Esc to cancel"),
        ];

        let info_text = clone_text.join("\n");

        // Highlight the input line differently
        let title_style = if can_confirm {
            Style::default().fg(Color::Green).bg(Color::Black)
        } else {
            Style::default().fg(Color::Yellow).bg(Color::Black)
        };

        let info_paragraph = Paragraph::new(info_text)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(" Clone ")
                    .style(title_style),
            )
            .style(Style::default().bg(Color::Black))
            .wrap(Wrap { trim: false });

        info_paragraph.render(popup_area, buf);

        // Render cursor at end of input (always on the new path line)
        // Y position: border (1) + title line (1) + blank (1) + original lines + blank (1) = new path line
        let input_y = popup_area.y + 1 + 1 + original_lines + 1;
        // X position: border (1) + prefix + visible input length
        #[allow(clippy::cast_possible_truncation)]
        let cursor_x = popup_area.x + 1 + new_path_display.len() as u16;

        // Show cursor (blinking effect via underscore)
        if cursor_x < popup_area.x + popup_area.width - 1
            && input_y < popup_area.y + popup_area.height - 1
        {
            buf[(cursor_x, input_y)].set_char('▏');
            buf[(cursor_x, input_y)].set_style(Style::default().fg(Color::White).bg(Color::Black));
        }
    }

    /// Render the clone progress popup.
    fn render_clone_progress_popup(&self, area: Rect, buf: &mut Buffer) {
        if let Some(progress) = &self.clone_progress {
            // Calculate popup size
            let popup_width = (area.width * 3 / 4).min(70);
            let popup_height = 10;

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

            let mut progress_lines = vec!["Cloning in progress...".to_string(), String::new()];

            if !progress.current_file.is_empty() {
                progress_lines.push(format!("Current: {}", progress.current_file));
            }

            progress_lines.push(format!(
                "Files: {} / {}",
                progress.files_completed, progress.total_files
            ));

            // Add error message if present
            if let Some(error) = &progress.error_message {
                progress_lines.push(String::new());
                progress_lines.push(format!("Error: {error}"));
            }

            let info_text = progress_lines.join("\n");
            let info_paragraph = Paragraph::new(info_text)
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(" Clone Progress ")
                        .style(Style::default().fg(Color::Yellow).bg(Color::Black)),
                )
                .style(Style::default().bg(Color::Black));

            info_paragraph.render(popup_area, buf);
        }
    }

    /// Render the delete confirmation dialog popup.
    fn render_delete_dialog_popup(&self, area: Rect, buf: &mut Buffer) {
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

        let item_type = if self.delete_is_folder {
            "folder"
        } else {
            "blob"
        };
        let can_confirm = self.delete_input == self.delete_target_name;

        let enter_hint = if can_confirm {
            "Enter to confirm"
        } else {
            "Type name to confirm"
        };

        let warning = if self.delete_is_folder {
            "⚠ This will delete all blobs in this folder!"
        } else {
            "⚠ This action cannot be undone!"
        };

        let delete_text = [
            format!("Delete {item_type}: {}", self.delete_target_name),
            String::new(),
            warning.to_string(),
            String::new(),
            format!("Type \"{}\" to confirm:", self.delete_target_name),
            self.delete_input.clone(),
            String::new(),
            format!("{enter_hint} • Esc to cancel"),
        ];

        let info_text = delete_text.join("\n");

        let title_style = if can_confirm {
            Style::default().fg(Color::Red).bg(Color::Black)
        } else {
            Style::default().fg(Color::Yellow).bg(Color::Black)
        };

        let info_paragraph = Paragraph::new(info_text)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(" Delete ")
                    .style(title_style),
            )
            .style(Style::default().bg(Color::Black));

        info_paragraph.render(popup_area, buf);

        // Render the input field with cursor
        let input_y = popup_area.y + 6;
        let input_x = popup_area.x + 1; // After left border
        #[allow(clippy::cast_possible_truncation)]
        let cursor_x = input_x + self.delete_input.len() as u16;

        // Show cursor
        if cursor_x < popup_area.x + popup_area.width - 1 {
            buf[(cursor_x, input_y)].set_char('▏');
            buf[(cursor_x, input_y)].set_style(Style::default().fg(Color::White).bg(Color::Black));
        }
    }

    /// Render the delete progress popup.
    fn render_delete_progress_popup(&self, area: Rect, buf: &mut Buffer) {
        if let Some(progress) = &self.delete_progress {
            // Calculate popup size
            let popup_width = (area.width * 3 / 4).min(70);
            let popup_height = 10;

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

            let mut progress_lines = vec!["Deleting...".to_string(), String::new()];

            if !progress.current_file.is_empty() {
                progress_lines.push(format!("Current: {}", progress.current_file));
            }

            progress_lines.push(format!(
                "Files: {} / {}",
                progress.files_completed, progress.total_files
            ));

            // Add error message if present
            if let Some(error) = &progress.error_message {
                progress_lines.push(String::new());
                progress_lines.push(format!("Error: {error}"));
            }

            let info_text = progress_lines.join("\n");
            let info_paragraph = Paragraph::new(info_text)
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(" Delete Progress ")
                        .style(Style::default().fg(Color::Red).bg(Color::Black)),
                )
                .style(Style::default().bg(Color::Black));

            info_paragraph.render(popup_area, buf);
        }
    }

    /// Render the preview panel for CSV, TSV, or JSON files.
    fn render_preview_panel(&self, area: Rect, buf: &mut Buffer) {
        // Get the file type name for the title
        let file_type_name = self
            .preview_file_type
            .as_ref()
            .map(|ft| ft.display_name())
            .unwrap_or_else(|| "Preview".to_string());

        // Handle loading state
        if self.is_loading_preview {
            let loading = Paragraph::new(format!("{} Loading preview...", self.icons.loading))
                .block(
                    Block::bordered()
                        .title(format!(" {} Preview ", file_type_name))
                        .title_alignment(Alignment::Center)
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
                    Block::bordered()
                        .title(format!(" {} Preview ", file_type_name))
                        .title_alignment(Alignment::Center)
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
                self.render_table_preview(area, buf, table, &file_type_name);
            }
            Some(PreviewData::Json(json)) => {
                self.render_json_preview(area, buf, json);
            }
            Some(PreviewData::Text(text)) => {
                self.render_text_preview(area, buf, text);
            }
            None => {
                let empty = Paragraph::new("No preview data available")
                    .block(
                        Block::bordered()
                            .title(format!(" {} Preview ", file_type_name))
                            .title_alignment(Alignment::Center)
                            .border_type(BorderType::Rounded),
                    )
                    .fg(Color::DarkGray)
                    .alignment(Alignment::Center);
                empty.render(area, buf);
            }
        }
    }

    /// Render a table preview (for CSV, TSV, or JSON array of objects).
    #[allow(clippy::cast_possible_truncation)]
    fn render_table_preview(
        &self,
        area: Rect,
        buf: &mut Buffer,
        table_data: &crate::preview::TablePreview,
        file_type_name: &str,
    ) {
        // Build title with row count info
        let title = if table_data.truncated {
            format!(
                " {} Preview ({}/{} rows, truncated) ",
                file_type_name,
                table_data.rows.len(),
                table_data.total_rows
            )
        } else {
            format!(
                " {} Preview ({} rows) ",
                file_type_name,
                table_data.rows.len()
            )
        };

        // Calculate column widths based on content
        let num_cols = table_data
            .headers
            .len()
            .max(table_data.rows.first().map(|r| r.len()).unwrap_or(0));

        if num_cols == 0 {
            let empty = Paragraph::new("Empty table")
                .block(
                    Block::bordered()
                        .title(title)
                        .title_alignment(Alignment::Center)
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

        // Consider data widths (sample first 50 rows for efficiency)
        for row in table_data.rows.iter().take(50) {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Cap column widths and apply horizontal scroll offset
        let col_offset = self.preview_scroll.1;
        let visible_cols: Vec<usize> = col_widths
            .iter()
            .skip(col_offset)
            .map(|w| (*w).clamp(3, 30)) // Min 3, max 30 chars per column
            .collect();

        // Build constraints for visible columns
        let constraints: Vec<Constraint> = visible_cols
            .iter()
            .map(|w| Constraint::Length((*w as u16) + 2)) // +2 for padding
            .collect();

        // Build header row
        let header_cells: Vec<Cell> = table_data
            .headers
            .iter()
            .skip(col_offset)
            .enumerate()
            .map(|(i, h)| {
                let display = if h.len() > visible_cols.get(i).copied().unwrap_or(30) {
                    format!("{}…", &h[..visible_cols.get(i).copied().unwrap_or(30) - 1])
                } else {
                    h.clone()
                };
                Cell::from(display).style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect();

        let header_row = Row::new(header_cells).height(1);

        // Build data rows with scroll offset
        let rows: Vec<Row> = table_data
            .rows
            .iter()
            .enumerate()
            .map(|(row_idx, row)| {
                let cells: Vec<Cell> = row
                    .iter()
                    .skip(col_offset)
                    .enumerate()
                    .map(|(i, cell)| {
                        let max_len = visible_cols.get(i).copied().unwrap_or(30);
                        let display = if cell.len() > max_len {
                            format!("{}…", &cell[..max_len - 1])
                        } else {
                            cell.clone()
                        };
                        Cell::from(display)
                    })
                    .collect();

                let style = if row_idx == self.preview_selected_row {
                    Style::default().bg(Color::DarkGray).fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::White)
                };

                Row::new(cells).style(style).height(1)
            })
            .collect();

        // Create the table widget
        let table = Table::new(rows, constraints)
            .header(header_row)
            .block(
                Block::bordered()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .row_highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));

        // Use TableState for scrolling
        let mut table_state = TableState::default();
        table_state.select(Some(self.preview_selected_row));

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
                Block::bordered()
                    .title(title)
                    .title_alignment(Alignment::Center)
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
                Block::bordered()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .fg(Color::White)
            .wrap(Wrap { trim: false });

        text_widget.render(area, buf);
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
