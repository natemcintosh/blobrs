use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Widget},
};

use crate::app::{App, AppState};

impl Widget for &App {
    /// Renders the user interface widgets.
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.state {
            AppState::ContainerSelection => self.render_container_selection(area, buf),
            AppState::BlobBrowsing => self.render_blob_browsing(area, buf),
        }
    }
}

impl App {
    fn render_container_selection(&self, area: Rect, buf: &mut Buffer) {
        // Create a vertical layout
        let mut constraints = vec![
            Constraint::Min(0), // Main content area
        ];

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

        constraints.push(Constraint::Length(3)); // Footer for instructions

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
            vec![ListItem::new(format!(
                "{} No containers found",
                self.icons.empty
            ))]
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

        let title = format!(
            " Azure Storage Account: {} - Select Container ",
            self.storage_account
        );

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

        // Footer with instructions
        let instructions = "Press `Esc`, `Ctrl-C` or `q` to quit • `r`/`F5` to refresh • `↑`/`↓` or `k`/`j` to navigate • `→`/`l`/`Enter` to select container";
        let footer = Paragraph::new(instructions)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .fg(Color::Cyan)
            .alignment(Alignment::Center);

        footer.render(chunks[chunk_index], buf);
    }

    fn render_blob_browsing(&self, area: Rect, buf: &mut Buffer) {
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

        constraints.push(Constraint::Length(3)); // Footer for instructions

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
                " Container: {} - {} [SEARCH] ",
                container_name, current_path_display
            )
        } else {
            format!(" Container: {} - {} ", container_name, current_path_display)
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

        // Footer with instructions
        let instructions = if self.search_mode {
            "Search Mode: Type to filter • `Enter` to confirm • `Esc` to cancel • `Ctrl+↑`/`Ctrl+↓` to navigate"
        } else {
            "Press `Esc`, `Ctrl-C` or `q` to quit • `r`/`F5` to refresh • `↑`/`↓` or `k`/`j` to navigate • `→`/`l`/`Enter` to enter folder • `←`/`h` to go up • `/` to search • `Backspace` to change container"
        };
        let footer = Paragraph::new(instructions)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .fg(Color::Cyan)
            .alignment(Alignment::Center);

        footer.render(chunks[chunk_index], buf);
    }
}
