use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

pub const MAX_HISTORY_LINES: usize = 1000;

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

// Terminal state structure with unified buffer
#[derive(Clone)]
pub struct TerminalState {
    // Unified buffer: all history + current screen in one VecDeque
    pub main_buffer: VecDeque<Vec<TerminalCell>>,
    pub visible_start: usize, // Start of currently visible area

    // Legacy compatibility (will point to visible area of main_buffer)
    pub screen: Vec<Vec<TerminalCell>>,
    pub scrollback: VecDeque<Vec<TerminalCell>>,

    pub cursor_row: usize, // Relative to visible area
    pub cursor_col: usize,
    pub rows: usize, // Visible rows
    pub cols: usize,
    pub current_color: AnsiColor,

    pub arrow_key_pressed: bool,
    pub arrow_key_time: Option<Instant>,

    // Alternative screen buffer (separate from main buffer)
    pub alt_screen: Vec<Vec<TerminalCell>>,
    pub is_alt_screen: bool,
    pub saved_cursor_main: (usize, usize),
    pub saved_cursor_alt: (usize, usize),
    pub cursor_visible: bool,
}

impl TerminalState {
    pub fn new(rows: usize, cols: usize) -> Self {
        let mut main_buffer = VecDeque::with_capacity(MAX_HISTORY_LINES + rows);

        // Initialize with empty rows for the initial screen
        for _ in 0..rows {
            main_buffer.push_back(vec![TerminalCell::default(); cols]);
        }

        let screen = vec![vec![TerminalCell::default(); cols]; rows];
        let alt_screen = vec![vec![TerminalCell::default(); cols]; rows];

        Self {
            main_buffer,
            visible_start: 0,
            screen,
            scrollback: VecDeque::new(), // Keep for compatibility, but won't be used
            cursor_row: 0,
            cursor_col: 0,
            rows,
            cols,
            current_color: AnsiColor::default(),
            arrow_key_pressed: false,
            arrow_key_time: None,
            alt_screen,
            is_alt_screen: false,
            saved_cursor_main: (0, 0),
            saved_cursor_alt: (0, 0),
            cursor_visible: true,
        }
    }

    // Update the legacy screen reference to point to visible area
    fn update_screen_reference(&mut self) {
        if self.is_alt_screen {
            self.screen = self.alt_screen.clone();
        } else {
            // Point screen to the visible portion of main_buffer
            self.screen.clear();
            for i in 0..self.rows {
                let buffer_index = self.visible_start + i;
                if buffer_index < self.main_buffer.len() {
                    self.screen.push(self.main_buffer[buffer_index].clone());
                } else {
                    self.screen.push(vec![TerminalCell::default(); self.cols]);
                }
            }
        }
    }

    // Ensure we're always at the bottom when new content arrives
    fn ensure_at_bottom(&mut self) {
        if !self.is_alt_screen && self.main_buffer.len() >= self.rows {
            self.visible_start = self.main_buffer.len() - self.rows;
            self.update_screen_reference();
        }
    }

    pub fn clear_screen(&mut self) {
        // Completely clear main_buffer and reset everything
        self.main_buffer.clear();

        // Create a fresh screen with current size
        for _ in 0..self.rows {
            self.main_buffer
                .push_back(vec![TerminalCell::default(); self.cols]);
        }

        self.visible_start = 0;
        self.update_screen_reference();
        self.cursor_row = 0;
        self.cursor_col = 0;

        println!("🧹 CLEARED: main_buffer completely emptied (Ctrl+L)");
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;

        // Calculate content before resize for debugging
        let content_before = if self.is_alt_screen {
            self.alt_screen
                .iter()
                .flatten()
                .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
                .count()
        } else {
            self.main_buffer
                .iter()
                .flatten()
                .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
                .count()
        };

        println!(
            "📊 Content before resize: {} chars, alt_screen: {}",
            content_before, self.is_alt_screen
        );

        if self.is_alt_screen {
            // For alt screen, just recreate with new dimensions
            self.alt_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];
        } else {
            // For main screen, preserve content during resize

            // First, ensure we have enough rows in main_buffer to accommodate new size
            while self.main_buffer.len() < new_rows {
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); old_cols]);
            }

            // Handle column resizing more carefully to preserve content
            if new_cols != old_cols {
                for row in &mut self.main_buffer {
                    if new_cols > old_cols {
                        // Expanding: add empty cells to the right
                        row.resize(new_cols, TerminalCell::default());
                    } else {
                        // Shrinking: preserve as much content as possible
                        // Instead of truncating, we could wrap content to next lines,
                        // but for simplicity, let's preserve what fits
                        if row.len() > new_cols {
                            // Simply truncate to new_cols to avoid complexity
                            row.truncate(new_cols);
                        }
                        // Ensure the row has exactly new_cols elements
                        row.resize(new_cols, TerminalCell::default());
                    }
                }
            }

            // Adjust visible_start to keep content visible
            if self.main_buffer.len() >= new_rows {
                // Try to keep the cursor visible and show recent content
                let cursor_absolute_row = self.visible_start + self.cursor_row;

                // Keep most recent content visible (bottom-aligned approach)
                if cursor_absolute_row + (new_rows / 4) < self.main_buffer.len() {
                    self.visible_start = self.main_buffer.len() - new_rows;
                } else {
                    self.visible_start =
                        cursor_absolute_row.saturating_sub(new_rows.saturating_sub(1));
                }

                // Ensure visible_start doesn't exceed buffer bounds
                self.visible_start = self
                    .visible_start
                    .min(self.main_buffer.len().saturating_sub(new_rows));
            } else {
                self.visible_start = 0;
            }
        }

        // Calculate current cursor absolute position before adjusting visible_start
        let old_cursor_absolute_row = if self.is_alt_screen {
            self.cursor_row // Alt screen cursor is already absolute
        } else {
            self.visible_start + self.cursor_row // Main screen cursor is relative to visible_start
        };

        // Ensure cursor position is still valid after resize
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));

        // Adjust cursor_row based on the new visible_start (for main screen only)
        if !self.is_alt_screen {
            // Calculate new cursor_row relative to new visible_start
            if old_cursor_absolute_row >= self.visible_start {
                self.cursor_row = old_cursor_absolute_row - self.visible_start;
            } else {
                // Cursor was above visible area, place it at top
                self.cursor_row = 0;
            }
            // Ensure cursor_row is within bounds
            self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));

            // IMPORTANT: Move cursor to a safe position to protect existing content
            // When shell receives SIGWINCH, it will clear from cursor position to end
            // So we move cursor to the end of content to prevent data loss
            // BUT only do this if there's actual meaningful content to protect
            if self.main_buffer.len() > 0 {
                // Check if there's actually meaningful content in the buffer
                let has_meaningful_content = self
                    .main_buffer
                    .iter()
                    .any(|row| row.iter().any(|cell| cell.ch != ' ' && cell.ch != '\0'));

                if has_meaningful_content {
                    let last_content_row = self.main_buffer.len() - 1;
                    if last_content_row >= self.visible_start {
                        let safe_cursor_row = last_content_row - self.visible_start;
                        if safe_cursor_row < new_rows {
                            // Find the actual last row with content
                            let mut actual_last_content_row = None;
                            for (i, row) in self.main_buffer.iter().enumerate().rev() {
                                if row.iter().any(|cell| cell.ch != ' ' && cell.ch != '\0') {
                                    actual_last_content_row = Some(i);
                                    break;
                                }
                            }

                            if let Some(content_row) = actual_last_content_row {
                                if content_row >= self.visible_start {
                                    let content_cursor_row = content_row - self.visible_start;
                                    if content_cursor_row < new_rows {
                                        self.cursor_row = content_cursor_row;
                                        // Move to end of line or find last non-space character
                                        let mut last_char_col = 0;
                                        for (i, cell) in
                                            self.main_buffer[content_row].iter().enumerate()
                                        {
                                            if cell.ch != ' ' && cell.ch != '\0' {
                                                last_char_col = i + 1;
                                            }
                                        }
                                        self.cursor_col =
                                            last_char_col.min(new_cols.saturating_sub(1));
                                    }
                                }
                            }
                        }
                    }
                }
                // If no meaningful content, keep cursor where it was (don't move it)
            }
        } else {
            // For alt screen, just ensure bounds
            self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        }

        println!(
            "📐 RESIZE: {}x{} -> {}x{} (visible_start: {}, main_buffer.len: {}, cursor: {}:{} -> {}:{})",
            old_cols,
            old_rows,
            new_cols,
            new_rows,
            self.visible_start,
            self.main_buffer.len(),
            old_cursor_absolute_row,
            self.cursor_col,
            if self.is_alt_screen { self.cursor_row } else { self.visible_start + self.cursor_row },
            self.cursor_col
        );

        // Update the active screen based on the current screen mode
        self.update_screen_reference();

        // Update dimensions
        self.rows = new_rows;
        self.cols = new_cols;

        // Update saved cursor positions to stay within bounds
        self.saved_cursor_main.0 = self.saved_cursor_main.0.min(new_rows.saturating_sub(1));
        self.saved_cursor_main.1 = self.saved_cursor_main.1.min(new_cols.saturating_sub(1));

        // Alt cursor can always be reset to top-left for the clean buffer
        self.saved_cursor_alt = (0, 0);

        // Calculate content after resize for debugging
        let content_after = if self.is_alt_screen {
            self.alt_screen
                .iter()
                .flatten()
                .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
                .count()
        } else {
            self.main_buffer
                .iter()
                .flatten()
                .filter(|cell| cell.ch != ' ' && cell.ch != '\0')
                .count()
        };

        println!("📊 Content after resize: {} chars", content_after);
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
            let absolute_row = if self.is_alt_screen {
                self.cursor_row
            } else {
                self.visible_start + self.cursor_row
            };

            // Place the character with current color
            if absolute_row < self.main_buffer.len() {
                self.main_buffer[absolute_row][self.cursor_col] = TerminalCell {
                    ch,
                    color: self.current_color.clone(),
                };

                // For wide characters (width 2), mark the second cell as a continuation
                if char_width == 2 && self.cursor_col + 1 < self.cols {
                    self.main_buffer[absolute_row][self.cursor_col + 1] = TerminalCell {
                        ch: '\u{0000}', // Null char as continuation marker
                        color: self.current_color.clone(),
                    };
                }
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
            // Add new line at the bottom, maintain history limit
            self.main_buffer
                .push_back(vec![TerminalCell::default(); self.cols]);

            // Trim if exceeds maximum history
            while self.main_buffer.len() > MAX_HISTORY_LINES {
                self.main_buffer.pop_front();
            }

            self.cursor_row = self.rows - 1;
            self.ensure_at_bottom();
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
                let absolute_row = if self.is_alt_screen {
                    self.cursor_row
                } else {
                    self.visible_start + self.cursor_row
                };

                while delete_col > 0
                    && absolute_row < self.main_buffer.len()
                    && delete_col < self.main_buffer[absolute_row].len()
                    && self.main_buffer[absolute_row][delete_col].ch == '\u{0000}'
                {
                    delete_col -= 1;
                }

                // Double-check we're still in user input area after finding the actual character
                if delete_col >= prompt_end
                    && absolute_row < self.main_buffer.len()
                    && delete_col < self.main_buffer[absolute_row].len()
                {
                    // Get the character we're about to delete
                    let ch_to_delete = self.main_buffer[absolute_row][delete_col].ch;
                    let char_width = ch_to_delete.width().unwrap_or(1);

                    // Clear the character and any continuation markers
                    for i in 0..char_width {
                        if delete_col + i < self.cols
                            && delete_col + i < self.main_buffer[absolute_row].len()
                        {
                            self.main_buffer[absolute_row][delete_col + i] =
                                TerminalCell::default();
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
            // Save current cursor position for main screen
            self.saved_cursor_main = (self.cursor_row, self.cursor_col);

            // Switch to alternative screen (start with clean screen)
            self.is_alt_screen = true;
            self.cursor_row = 0;
            self.cursor_col = 0;
            self.update_screen_reference();

            println!("🔄 Switched to alternative screen buffer (clean screen)");
        }
    }

    // Switch back to main screen buffer
    pub fn switch_to_main_screen(&mut self) {
        if self.is_alt_screen {
            // Restore main screen and cursor position
            self.is_alt_screen = false;
            self.cursor_row = self.saved_cursor_main.0;
            self.cursor_col = self.saved_cursor_main.1;
            self.update_screen_reference();

            println!("🔄 Restored main screen buffer");
        }
    }
}
