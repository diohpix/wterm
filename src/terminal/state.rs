use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

pub const MAX_SCROLLBACK: usize = 1000;

// ANSI 색상 정보를 저장하는 구조체
#[derive(Clone, Debug, PartialEq)]
pub struct AnsiColor {
    pub foreground: egui::Color32,
    pub background: egui::Color32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

impl Default for AnsiColor {
    fn default() -> Self {
        Self {
            foreground: egui::Color32::from_rgb(203, 204, 205), // Terminal white
            background: egui::Color32::TRANSPARENT,
            bold: false,
            italic: false,
            underline: false,
            reverse: false,
        }
    }
}

// 터미널 셀 정보 (문자 + 색상)
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCell {
    pub ch: char,
    pub color: AnsiColor,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            color: AnsiColor::default(),
        }
    }
}

// Terminal state structure
#[derive(Clone)]
pub struct TerminalState {
    pub screen: Vec<Vec<TerminalCell>>,
    pub scrollback: VecDeque<Vec<TerminalCell>>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub rows: usize,
    pub cols: usize,
    pub current_color: AnsiColor, // 현재 색상 상태

    pub arrow_key_pressed: bool, // Track if arrow key was recently pressed
    pub arrow_key_time: Option<Instant>, // When arrow key was last pressed
    // Alternative screen buffer support
    pub main_screen: Vec<Vec<TerminalCell>>, // Main screen buffer
    pub main_scrollback: VecDeque<Vec<TerminalCell>>, // Saved scrollback for main screen
    pub alt_screen: Vec<Vec<TerminalCell>>,  // Alternative screen buffer
    pub is_alt_screen: bool,                 // Currently using alternative screen
    pub saved_cursor_main: (usize, usize),   // Saved cursor position for main screen
    pub saved_cursor_alt: (usize, usize),    // Saved cursor position for alt screen
    pub cursor_visible: bool,                // Is the cursor currently visible?
}

impl TerminalState {
    pub fn new(rows: usize, cols: usize) -> Self {
        let screen = vec![vec![TerminalCell::default(); cols]; rows];
        let main_screen = vec![vec![TerminalCell::default(); cols]; rows];
        let alt_screen = vec![vec![TerminalCell::default(); cols]; rows];
        Self {
            screen,
            scrollback: VecDeque::with_capacity(MAX_SCROLLBACK),
            cursor_row: 0,
            cursor_col: 0,
            rows,
            cols,
            current_color: AnsiColor::default(),
            arrow_key_pressed: false,
            arrow_key_time: None,
            main_screen,
            main_scrollback: VecDeque::new(),
            alt_screen,
            is_alt_screen: false,
            saved_cursor_main: (0, 0),
            saved_cursor_alt: (0, 0),
            cursor_visible: true,
        }
    }

    pub fn clear_screen(&mut self) {
        // Move all non-empty lines from the screen to the scrollback buffer, regardless of screen mode
        for row in self.screen.iter().filter(|r| r.iter().any(|c| c.ch != ' ')) {
            self.scrollback.push_back(row.clone());
        }
        // Trim scrollback if it exceeds the maximum size
        while self.scrollback.len() > MAX_SCROLLBACK {
            self.scrollback.pop_front();
        }

        // Clear all content and reset cursor to top-left
        for row in &mut self.screen {
            for cell in row {
                *cell = TerminalCell::default();
            }
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;

        // 1. Resize main_screen, preserving content from the bottom
        let mut new_main_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];
        let rows_to_copy = std::cmp::min(new_rows, old_rows);
        let cols_to_copy = std::cmp::min(new_cols, old_cols);
        let old_screen_start = old_rows.saturating_sub(rows_to_copy);
        let new_screen_start = new_rows.saturating_sub(rows_to_copy);

        for r in 0..rows_to_copy {
            for c in 0..cols_to_copy {
                new_main_screen[new_screen_start + r][c] =
                    self.main_screen[old_screen_start + r][c].clone();
            }
        }
        self.main_screen = new_main_screen;

        // 2. Recreate alt_screen (no need to preserve content)
        self.alt_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];

        // 3. Update the active screen based on the current screen mode
        if self.is_alt_screen {
            self.screen = self.alt_screen.clone();
        } else {
            self.screen = self.main_screen.clone();
        }

        // 4. Update dimensions
        self.rows = new_rows;
        self.cols = new_cols;

        // 5. Adjust cursor positions to stay within bounds and follow content
        let row_offset = old_rows.saturating_sub(new_rows);

        self.cursor_row = self
            .cursor_row
            .saturating_sub(row_offset)
            .min(new_rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));

        self.saved_cursor_main.0 = self
            .saved_cursor_main
            .0
            .saturating_sub(row_offset)
            .min(new_rows.saturating_sub(1));
        self.saved_cursor_main.1 = self.saved_cursor_main.1.min(new_cols.saturating_sub(1));

        // Alt cursor can always be reset to top-left for the clean buffer
        self.saved_cursor_alt = (0, 0);
    }

    pub fn put_char(&mut self, ch: char) {
        // Reset arrow key state when adding new text
        self.clear_arrow_key_protection();

        // Get the display width of the character
        let char_width = ch.width().unwrap_or(1);

        // Check if we have enough space for this character
        if self.cursor_col + char_width > self.cols {
            self.newline();
        }

        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            // Place the character with current color
            self.screen[self.cursor_row][self.cursor_col] = TerminalCell {
                ch,
                color: self.current_color.clone(),
            };

            // For wide characters (width 2), mark the second cell as a continuation
            if char_width == 2 && self.cursor_col + 1 < self.cols {
                self.screen[self.cursor_row][self.cursor_col + 1] = TerminalCell {
                    ch: '\u{0000}', // Null char as continuation marker
                    color: self.current_color.clone(),
                };
            }

            // Move cursor by the character width
            self.cursor_col += char_width;
        }

        // If we've reached the end of the line, wrap to next line
        if self.cursor_col >= self.cols {
            self.newline();
        }
    }

    pub fn newline(&mut self) {
        // Reset arrow key state when moving to new line
        self.clear_arrow_key_protection();

        self.cursor_row += 1;
        self.cursor_col = 0;
        if self.cursor_row >= self.rows {
            // Scroll up: move the top line of the screen to the scrollback buffer
            self.scrollback.push_back(self.screen.remove(0));
            if self.scrollback.len() > MAX_SCROLLBACK {
                self.scrollback.pop_front();
            }

            self.screen.push(vec![TerminalCell::default(); self.cols]);
            self.cursor_row = self.rows - 1;
        }
    }

    pub fn carriage_return(&mut self) {
        self.cursor_col = 0;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            // Find prompt end to prevent deleting into prompt area
            let mut prompt_end = 0;
            if self.cursor_row < self.screen.len() {
                let row = &self.screen[self.cursor_row];
                // Find prompt end: "~ " or "✗ " pattern
                for i in 0..row.len().saturating_sub(1) {
                    if (row[i].ch == '~' || row[i].ch == '✗') && row[i + 1].ch == ' ' {
                        prompt_end = i + 2; // Position after "~ " or "✗ "
                        break;
                    }
                }
            }

            // Only allow backspace if cursor is beyond prompt area
            if self.cursor_col > prompt_end {
                // Move cursor back to find the character to delete
                let mut delete_col = self.cursor_col - 1;

                // If we're on a continuation marker (\u{0000}), move back to the actual character
                while delete_col > 0 && self.screen[self.cursor_row][delete_col].ch == '\u{0000}' {
                    delete_col -= 1;
                }

                // Double-check we're still in user input area after finding the actual character
                if delete_col >= prompt_end {
                    // Get the character we're about to delete
                    let ch_to_delete = self.screen[self.cursor_row][delete_col].ch;
                    let char_width = ch_to_delete.width().unwrap_or(1);

                    // Clear the character and any continuation markers
                    for i in 0..char_width {
                        if delete_col + i < self.cols {
                            self.screen[self.cursor_row][delete_col + i] = TerminalCell::default();
                        }
                    }

                    // Move cursor to the position of the deleted character
                    self.cursor_col = delete_col;
                }
            }
        }
    }

    pub fn move_cursor_to(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
    }

    // Check if arrow key protection should still be active (within 300ms)
    pub fn should_protect_from_arrow_key(&self) -> bool {
        if !self.arrow_key_pressed {
            return false;
        }

        if let Some(arrow_time) = self.arrow_key_time {
            let elapsed = arrow_time.elapsed();
            elapsed.as_millis() < 300 // 300ms protection window to catch delayed backspaces
        } else {
            false
        }
    }

    // Set arrow key protection with current timestamp
    pub fn set_arrow_key_protection(&mut self) {
        self.arrow_key_pressed = true;
        self.arrow_key_time = Some(Instant::now());
    }

    // Clear arrow key protection
    pub fn clear_arrow_key_protection(&mut self) {
        self.arrow_key_pressed = false;
        self.arrow_key_time = None;
    }

    // Switch to alternative screen buffer
    pub fn switch_to_alt_screen(&mut self) {
        if !self.is_alt_screen {
            // Save current main screen state
            self.main_screen = self.screen.clone();
            self.main_scrollback = self.scrollback.clone();
            self.saved_cursor_main = (self.cursor_row, self.cursor_col);

            // Switch to alternative screen (start with clean screen)
            // Create a completely clean alt screen buffer
            self.screen = self.alt_screen.clone();
            self.scrollback.clear(); // Alt screen has no scrollback
            self.cursor_row = 0;
            self.cursor_col = 0;
            self.is_alt_screen = true;

            println!("🔄 Switched to alternative screen buffer (clean screen)");
        }
    }

    // Switch back to main screen buffer
    pub fn switch_to_main_screen(&mut self) {
        if self.is_alt_screen {
            // Don't save alt screen state - each app gets a clean alt screen
            // Just restore main screen
            self.screen = self.main_screen.clone();
            self.scrollback = self.main_scrollback.clone();
            self.cursor_row = self.saved_cursor_main.0;
            self.cursor_col = self.saved_cursor_main.1;
            self.is_alt_screen = false;

            println!("🔄 Restored main screen buffer");
        }
    }
}
