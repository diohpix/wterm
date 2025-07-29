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
            foreground: egui::Color32::WHITE, // Pure white for better contrast
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

// Terminal state structure with unified virtual screen
#[derive(Clone)]
pub struct TerminalState {
    // Unified virtual screen - all content in one continuous buffer
    pub virtual_screen: Vec<Vec<TerminalCell>>,
    // Alternative screen for apps like vim (separate virtual screen)
    pub alt_virtual_screen: Vec<Vec<TerminalCell>>,

    // Cursor position (absolute coordinates in virtual_screen)
    pub cursor_row: usize,
    pub cursor_col: usize,

    // Viewport (what's currently visible)
    pub viewport_start: usize, // First visible row in virtual_screen
    pub rows: usize,           // Number of visible rows
    pub cols: usize,           // Number of columns

    // Terminal state
    pub current_color: AnsiColor,
    pub cursor_visible: bool,
    pub is_alt_screen: bool,
    pub saved_cursor_main: (usize, usize), // Absolute coordinates
    pub saved_cursor_alt: (usize, usize),  // Absolute coordinates

    // Arrow key protection
    pub arrow_key_pressed: bool,
    pub arrow_key_time: Option<Instant>,
}

impl TerminalState {
    pub fn new(rows: usize, cols: usize) -> Self {
        // Start with initial viewport size, but virtual screen can grow
        let virtual_screen = vec![vec![TerminalCell::default(); cols]; rows];
        let alt_virtual_screen = vec![vec![TerminalCell::default(); cols]; rows];

        Self {
            virtual_screen,
            alt_virtual_screen,
            cursor_row: 0,
            cursor_col: 0,
            viewport_start: 0,
            rows,
            cols,
            current_color: AnsiColor::default(),
            cursor_visible: true,
            is_alt_screen: false,
            saved_cursor_main: (0, 0),
            saved_cursor_alt: (0, 0),
            arrow_key_pressed: false,
            arrow_key_time: None,
        }
    }

    pub fn clear_screen(&mut self) {
        if self.is_alt_screen {
            // In alt screen, just clear all content
            for row in &mut self.alt_virtual_screen {
                for cell in row {
                    *cell = TerminalCell::default();
                }
            }
            self.cursor_row = self.viewport_start;
        } else {
            // In main screen, clear the visible area only
            let visible_end = (self.viewport_start + self.rows).min(self.virtual_screen.len());
            for row_idx in self.viewport_start..visible_end {
                for cell in &mut self.virtual_screen[row_idx] {
                    *cell = TerminalCell::default();
                }
            }
            self.cursor_row = self.viewport_start;
        }
        self.cursor_col = 0;
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;

        // Store scrollback content count for debugging
        let scrollback_lines = self.scrollback.len();
        let main_content = self
            .main_screen
            .iter()
            .flatten()
            .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
            .count();
        let current_screen_content = self
            .screen
            .iter()
            .flatten()
            .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
            .count();
        println!(
            "🗂️ Before resize: scrollback={} lines, main_screen={} chars, current_screen={} chars, is_alt={}",
            scrollback_lines, main_content, current_screen_content, self.is_alt_screen
        );

        // 1. Handle scrollback preservation during resize
        if new_rows < old_rows && !self.is_alt_screen {
            // Terminal is getting smaller - move excess content to scrollback
            let excess_rows = old_rows - new_rows;
            let mut moved_lines = 0;
            println!(
                "📦 Terminal shrinking: moving {} excess rows to scrollback",
                excess_rows
            );
            for i in 0..excess_rows {
                if i < self.main_screen.len() && self.main_screen[i].iter().any(|c| c.ch != ' ') {
                    // Only add non-empty lines to scrollback
                    self.scrollback.push_back(self.main_screen[i].clone());
                    moved_lines += 1;
                }
            }
            println!("📦 Moved {} non-empty lines to scrollback", moved_lines);
            // Trim scrollback if it exceeds maximum
            while self.scrollback.len() > MAX_SCROLLBACK {
                self.scrollback.pop_front();
            }
        }

        // 2. Resize the currently active screen (which may be main_screen or alt_screen)
        let mut new_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];
        let rows_to_copy = std::cmp::min(new_rows, old_rows);
        let cols_to_copy = std::cmp::min(new_cols, old_cols);

        println!(
            "🔄 Resize debug: old={}x{}, new={}x{}, copy={}x{}",
            old_cols, old_rows, new_cols, new_rows, cols_to_copy, rows_to_copy
        );

        // Copy from the most recent content (bottom-aligned)
        // We want to preserve the bottom content (most recent)
        let old_screen_start = if old_rows > rows_to_copy {
            old_rows - rows_to_copy // Start from the bottom part of old screen
        } else {
            0
        };
        let new_screen_start = if new_rows > rows_to_copy {
            new_rows - rows_to_copy // Place content at the bottom of new screen
        } else {
            0
        };

        println!(
            "📋 Copy range: old_start={}, new_start={}",
            old_screen_start, new_screen_start
        );

        for r in 0..rows_to_copy {
            for c in 0..cols_to_copy {
                if old_screen_start + r < self.screen.len()
                    && c < self.screen[old_screen_start + r].len()
                {
                    new_screen[new_screen_start + r][c] =
                        self.screen[old_screen_start + r][c].clone();
                }
            }
        }

        // Debug: Check if we preserved any content
        let preserved_chars = new_screen
            .iter()
            .flatten()
            .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
            .count();
        println!("📝 Preserved {} non-space characters", preserved_chars);

        // Update both main_screen and current screen
        if self.is_alt_screen {
            self.alt_screen = new_screen.clone();
            self.screen = new_screen;
        } else {
            self.main_screen = new_screen.clone();
            self.screen = new_screen;
        }

        // 3. Handle the other screen (main/alt) that's not currently active
        if self.is_alt_screen {
            // We're in alt screen, so reset the main_screen to match new dimensions
            self.main_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];
            println!("🔄 Resized main_screen while in alt_screen mode");
        } else {
            // We're in main screen, so reset the alt_screen to match new dimensions
            self.alt_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];
            println!("🔄 Resized alt_screen while in main_screen mode");
        }

        // 5. Update dimensions
        self.rows = new_rows;
        self.cols = new_cols;

        // 6. Final state check
        let final_scrollback = self.scrollback.len();
        let final_main_content = self
            .main_screen
            .iter()
            .flatten()
            .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
            .count();
        println!(
            "🏁 After resize: scrollback={} lines, main_screen={} chars",
            final_scrollback, final_main_content
        );

        // 7. Cursor positions will be handled by the calling function
        // Just ensure they're within bounds as fallback
        self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));

        // Update saved cursor positions
        self.saved_cursor_main.0 = self.saved_cursor_main.0.min(new_rows.saturating_sub(1));
        self.saved_cursor_main.1 = self.saved_cursor_main.1.min(new_cols.saturating_sub(1));

        self.saved_cursor_alt.0 = self.saved_cursor_alt.0.min(new_rows.saturating_sub(1));
        self.saved_cursor_alt.1 = self.saved_cursor_alt.1.min(new_cols.saturating_sub(1));
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
