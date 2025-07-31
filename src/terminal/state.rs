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

        // Start with just one empty row - more will be added as needed
        main_buffer.push_back(vec![TerminalCell::default(); cols]);

        let _screen = vec![vec![TerminalCell::default(); cols]; rows];
        let alt_screen = vec![vec![TerminalCell::default(); cols]; rows];

        Self {
            main_buffer,
            visible_start: 0,
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

    // Ensure we're always at the bottom when new content arrives
    fn ensure_at_bottom(&mut self) {
        if !self.is_alt_screen && self.main_buffer.len() >= self.rows {
            self.visible_start = self.main_buffer.len() - self.rows;
        }
    }

    pub fn clear_screen(&mut self) {
        // Completely clear main_buffer and reset everything
        self.main_buffer.clear();

        // Start with just one empty row - more will be added as needed
        self.main_buffer
            .push_back(vec![TerminalCell::default(); self.cols]);

        self.visible_start = 0;
        self.cursor_row = 0;
        self.cursor_col = 0;

        println!("🧹 CLEARED: main_buffer completely emptied (Ctrl+L)");
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }

        let _old_rows = self.rows;
        let old_cols = self.cols;

        self.rows = new_rows;
        self.cols = new_cols;

        if self.is_alt_screen {
            // For alt screen, just recreate with new dimensions
            self.alt_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];
        } else {
            // For main screen, preserve content during resize
            // Note: We no longer pre-allocate rows - they'll be added as needed

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
                println!("----- {}", self.main_buffer.len());
                // Try to keep the cursor visible and show recent content
                let cursor_absolute_row = self.visible_start + self.cursor_row;

                // Keep most recent content visible (bottom-aligned approach)
                if cursor_absolute_row < self.main_buffer.len() {
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
            println!("visible_start: {}", self.visible_start);
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
                // SUPER OPTIMIZED: Only check the visible area for meaningful content
                // This is much faster and logically correct - we only care about visible content for cursor positioning
                let visible_end = (self.visible_start + new_rows).min(self.main_buffer.len());

                let mut actual_last_content_row = None;
                for i in (self.visible_start..visible_end).rev() {
                    if self.main_buffer[i]
                        .iter()
                        .any(|cell| cell.ch != ' ' && cell.ch != '\0')
                    {
                        actual_last_content_row = Some(i);
                        break;
                    }
                }

                // If we found meaningful content, position cursor safely
                if let Some(content_row) = actual_last_content_row {
                    if content_row >= self.visible_start {
                        let content_cursor_row = content_row - self.visible_start;
                        if content_cursor_row < new_rows {
                            self.cursor_row = content_cursor_row;
                            // Move to end of line or find last non-space character
                            let mut last_char_col = 0;
                            for (i, cell) in self.main_buffer[content_row].iter().enumerate() {
                                if cell.ch != ' ' && cell.ch != '\0' {
                                    last_char_col = i + 1;
                                }
                            }
                            self.cursor_col = last_char_col.min(new_cols.saturating_sub(1));
                        }
                    }
                }
                // If no meaningful content found in recent history, keep cursor where it was
            }
        } else {
            // For alt screen, just ensure bounds
            self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        }

        // Update saved cursor positions to stay within bounds
        self.saved_cursor_main.0 = self.saved_cursor_main.0.min(new_rows.saturating_sub(1));
        self.saved_cursor_main.1 = self.saved_cursor_main.1.min(new_cols.saturating_sub(1));

        // Alt cursor can always be reset to top-left for the clean buffer
        self.saved_cursor_alt = (0, 0);

        // Calculate content after resize for debugging
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

            // Ensure we have enough rows in main_buffer
            while absolute_row >= self.main_buffer.len() {
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); self.cols]);
            }

            // Place the character with current color
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
            let absolute_row = if self.is_alt_screen {
                self.cursor_row
            } else {
                self.visible_start + self.cursor_row
            };

            let mut prompt_end = 0;
            if absolute_row < self.main_buffer.len() {
                let row = &self.main_buffer[absolute_row];
                for i in 0..row.len().saturating_sub(1) {
                    if (row[i].ch == '~' || row[i].ch == '✗') && row[i + 1].ch == ' ' {
                        prompt_end = i + 2;
                        break;
                    }
                }
            }

            if self.cursor_col > prompt_end {
                let mut delete_col = self.cursor_col - 1;

                while delete_col > 0
                    && absolute_row < self.main_buffer.len()
                    && delete_col < self.main_buffer[absolute_row].len()
                    && self.main_buffer[absolute_row][delete_col].ch == '\u{0000}'
                {
                    delete_col -= 1;
                }

                if delete_col >= prompt_end
                    && absolute_row < self.main_buffer.len()
                    && delete_col < self.main_buffer[absolute_row].len()
                {
                    let ch_to_delete = self.main_buffer[absolute_row][delete_col].ch;
                    let char_width = ch_to_delete.width().unwrap_or(1);

                    for i in 0..char_width {
                        if delete_col + i < self.cols
                            && delete_col + i < self.main_buffer[absolute_row].len()
                        {
                            self.main_buffer[absolute_row][delete_col + i] =
                                TerminalCell::default();
                        }
                    }

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

            println!("🔄 Restored main screen buffer");
        }
    }
}
