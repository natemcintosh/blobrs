use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Widget},
};

use crate::app::App;

impl Widget for &App {
    /// Renders the user interface widgets.
    ///
    // This is where you add new widgets.
    // See the following resources:
    // - https://docs.rs/ratatui/latest/ratatui/widgets/index.html
    // - https://github.com/ratatui/ratatui/tree/master/examples
    fn render(self, area: Rect, buf: &mut Buffer) {
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
            constraints.push(Constraint::Length(3)); // Error/loading area
        }

        constraints.push(Constraint::Length(3)); // Footer for instructions

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Main block with file list
        let file_items: Vec<ListItem> = if self.is_loading {
            vec![ListItem::new("🔄 Loading...")]
        } else if self.files.is_empty() {
            if self.search_mode && !self.search_query.is_empty() {
                vec![ListItem::new("� No results found")]
            } else {
                vec![ListItem::new("�📭 No items found")]
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

        let title = if self.search_mode {
            format!(" Azure Blob Storage - {} [SEARCH] ", current_path_display)
        } else {
            format!(" Azure Blob Storage - {} ", current_path_display)
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
                        .border_type(BorderType::Rounded)
                )
                .fg(Color::Cyan)
                .alignment(Alignment::Left);
            search_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        }

        // Error/Loading message if present
        if let Some(error) = &self.error_message {
            let error_widget = Paragraph::new(format!("❌ {}", error))
                .block(Block::bordered().border_type(BorderType::Rounded))
                .fg(Color::Red)
                .alignment(Alignment::Center);
            error_widget.render(chunks[chunk_index], buf);
            chunk_index += 1;
        } else if self.is_loading {
            let loading_widget = Paragraph::new("🔄 Loading Azure Blob Storage...")
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
            "Press `Esc`, `Ctrl-C` or `q` to quit • `r`/`F5` to refresh • `↑`/`↓` or `k`/`j` to navigate • `→`/`l`/`Enter` to enter folder • `←`/`h` to go up • `/` to search"
        };
        let footer = Paragraph::new(instructions)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .fg(Color::Cyan)
            .alignment(Alignment::Center);

        footer.render(chunks[chunk_index], buf);
    }
}
