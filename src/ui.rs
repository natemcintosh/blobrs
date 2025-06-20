use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    widgets::{Block, BorderType, List, ListItem, ListState, Widget},
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
        // Create a vertical layout with main content and footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),    // Main content area
                Constraint::Length(3), // Footer for instructions
            ])
            .split(area);

        // Main block with file list
        let file_items: Vec<ListItem> = self
            .files
            .iter()
            .map(|file| ListItem::new(file.as_str()))
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.selected_index));

        let main_block = List::new(file_items)
            .block(
                Block::bordered()
                    .title(format!(" Blobrs - {} ", self.current_dir))
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .fg(Color::Green)
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow))
            .highlight_symbol("▶ ");

        ratatui::widgets::StatefulWidget::render(main_block, chunks[0], buf, &mut list_state);

        // Footer with instructions
        let instructions = "Press `Esc`, `Ctrl-C` or `q` to quit • `r`/`F5` to refresh • `↑`/`↓` or `k`/`j` to navigate • `→`/`l`/`Enter` to enter folder • `←`/`h` to go up";
        let footer = ratatui::widgets::Paragraph::new(instructions)
            .block(Block::bordered().border_type(BorderType::Rounded))
            .fg(Color::Cyan)
            .alignment(Alignment::Center);

        footer.render(chunks[1], buf);
    }
}
