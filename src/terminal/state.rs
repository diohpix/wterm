use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

pub const MAX_HISTORY_LINES: usize = 1000;
pub const MAX_MAIN_BUFFER_COLS: usize = 1000; // Fixed width for main_buffer to preserve original data

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

// Terminal state structure with separated buffers
#[derive(Clone)]
pub struct TerminalState {
    // Main buffer: stores the logical lines of the terminal history.
    pub main_buffer: VecDeque<Vec<TerminalCell>>,

    // Render buffer: stores the visual lines after reflow.
    // This is what is actually displayed.
    pub render_buffer: Vec<Vec<TerminalCell>>,
    pub render_buffer_dirty: bool,

    // Logical cursor position in the main_buffer.
    pub cursor_row: usize,
    pub cursor_col: usize,

    // Visual cursor position in the render_buffer.
    // This is calculated by update_render_buffer.
    pub render_cursor_row: usize,
    pub render_cursor_col: usize,

    // Terminal dimensions (in characters).
    pub rows: usize,
    pub cols: usize,

    pub current_color: AnsiColor,
    pub arrow_key_pressed: bool,
    pub arrow_key_time: Option<Instant>,

    // Alternative screen mode (uses main_buffer but with screen size limits)
    pub is_alt_screen: bool,
    pub saved_cursor_main: (usize, usize),
    pub saved_cursor_alt: (usize, usize),
    pub cursor_visible: bool,

    // Backup for main buffer when switching to alt screen
    pub main_buffer_backup: Option<VecDeque<Vec<TerminalCell>>>,
}

impl TerminalState {
    // Find the actual end of text in a row (excluding trailing spaces)
    fn find_row_text_end(&self, row: &Vec<TerminalCell>) -> usize {
        row.iter()
            .rposition(|cell| cell.ch != ' ' && cell.ch != '\u{0000}')
            .map_or(0, |i| i + 1)
    }

    // Mark render_buffer as dirty for batch update
    pub fn mark_render_dirty(&mut self) {
        self.render_buffer_dirty = true;
    }

    // Update render_buffer from main_buffer's visible area (only if dirty)
    pub fn update_render_buffer_if_dirty(&mut self) {
        if self.render_buffer_dirty {
            self.update_render_buffer();
            self.render_buffer_dirty = false;
        }
    }

    // Update render buffer and calculate cursor offset in one pass (like commit 7a65ed9)
    pub fn update_render_buffer(&mut self) {
        // Clear render_buffer first
        self.render_buffer.clear();

        let mut main_buffer_idx = 0;

        // Process main_buffer rows and reflow them into render_buffer
        // Keep the original design: render_buffer processes all main_buffer for proper scrolling
        while main_buffer_idx < self.main_buffer.len() {
            let source_row = &self.main_buffer[main_buffer_idx];
            let is_cursor_row = main_buffer_idx == self.cursor_row;

            // Find the actual end of text in this row
            let text_end = self.find_row_text_end(source_row);

            // Check if this row needs reflow based on actual text length
            let needs_reflow = text_end > self.cols;

            if !needs_reflow {
                // Simple copy without reflow - only copy up to text end or cols
                let mut render_row = vec![TerminalCell::default(); self.cols];
                let copy_length = text_end.min(self.cols);
                if copy_length > 0 {
                    render_row[..copy_length].clone_from_slice(&source_row[..copy_length]);
                }
                self.render_buffer.push(render_row);

                // If this is cursor row, record the render row
                if is_cursor_row {
                    self.render_cursor_row = self.render_buffer.len() - 1;
                    self.render_cursor_col = self.cursor_col.min(self.cols);
                }
            } else {
                // Reflow: split long row across multiple render rows
                let mut source_col = 0;
                let cursor_render_start = self.render_buffer.len(); // Remember where this row starts

                while source_col < text_end {
                    let mut render_row = vec![TerminalCell::default(); self.cols];
                    let mut render_col = 0;

                    // Fill current render row up to cols width
                    while render_col < self.cols && source_col < text_end {
                        // Skip null characters (wide char continuations)
                        if source_row[source_col].ch == '\u{0000}' {
                            source_col += 1;
                            continue;
                        }

                        // Check if character fits in current render row
                        let char_width = source_row[source_col].ch.width().unwrap_or(1);
                        if render_col + char_width > self.cols {
                            break; // Move to next render row
                        }

                        render_row[render_col] = source_row[source_col].clone();

                        // For wide characters, mark the second cell as continuation
                        if char_width == 2 && render_col + 1 < self.cols {
                            render_row[render_col + 1] = TerminalCell {
                                ch: '\u{0000}',
                                color: source_row[source_col].color.clone(),
                            };
                        }

                        // Check if this is where cursor should be (for cursor row)
                        if is_cursor_row && source_col == self.cursor_col {
                            self.render_cursor_row = self.render_buffer.len();
                            self.render_cursor_col = render_col;
                        }

                        render_col += char_width;
                        source_col += 1;
                    }

                    self.render_buffer.push(render_row);
                }

                // If cursor was in this row but not found yet (at end of line or beyond),
                // place it at the last render row for this main_buffer row
                if is_cursor_row && self.cursor_col >= text_end {
                    self.render_cursor_row =
                        (self.render_buffer.len() - 1).max(cursor_render_start);
                    self.render_cursor_col = self.cols.saturating_sub(1);
                }
            }

            // Always move to next main_buffer row after processing
            main_buffer_idx += 1;
        }

        self.render_buffer_dirty = false;
    }

    pub fn new(rows: usize, cols: usize) -> Self {
        let mut main_buffer = VecDeque::with_capacity(MAX_HISTORY_LINES + rows);
        main_buffer.push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);

        let mut state = Self {
            main_buffer,
            render_buffer: Vec::new(),
            render_buffer_dirty: true,
            cursor_row: 0,
            cursor_col: 0,
            render_cursor_row: 0,
            render_cursor_col: 0,
            rows,
            cols,
            current_color: AnsiColor::default(),
            arrow_key_pressed: false,
            arrow_key_time: None,
            is_alt_screen: false,
            saved_cursor_main: (0, 0),
            saved_cursor_alt: (0, 0),
            cursor_visible: true,
            main_buffer_backup: None,
        };
        state.update_render_buffer();
        state
    }

    pub fn clear_screen(&mut self) {
        self.main_buffer.clear();
        self.main_buffer
            .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.mark_render_dirty();
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if self.rows == new_rows && self.cols == new_cols {
            return;
        }

        let _old_rows = self.rows;
        self.rows = new_rows;
        self.cols = new_cols;

        // In alt screen mode, don't force buffer size changes
        // Let the application (top, vim, etc.) handle resize by itself

        // When resizing, the entire buffer needs to be reflowed.
        self.mark_render_dirty();

        // Ensure cursor is within new bounds
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));

        // Ensure main_buffer has at least one row
        if self.main_buffer.is_empty() {
            self.main_buffer
                .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
        }

        self.cursor_row = self.cursor_row.min(self.main_buffer.len() - 1);
    }

    pub fn put_char(&mut self, ch: char) {
        self.clear_arrow_key_protection();
        let char_width = ch.width().unwrap_or(1);

        // Ensure row exists in main_buffer
        while self.cursor_row >= self.main_buffer.len() {
            self.main_buffer
                .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
        }

        // Additional safety check
        if self.cursor_row >= self.main_buffer.len() {
            eprintln!(
                "🚨 PANIC PREVENTION: cursor_row={}, buffer_len={}",
                self.cursor_row,
                self.main_buffer.len()
            );
            return; // Early return to prevent panic
        }

        let buffer = &mut self.main_buffer[self.cursor_row];

        // Ensure row has enough capacity
        if self.cursor_col + char_width >= buffer.len() {
            buffer.resize(self.cursor_col + char_width, TerminalCell::default());
        }

        buffer[self.cursor_col] = TerminalCell {
            ch,
            color: self.current_color.clone(),
        };

        if char_width == 2 {
            if self.cursor_col + 1 < buffer.len() {
                buffer[self.cursor_col + 1] = TerminalCell {
                    ch: '\u{0000}', // Continuation marker
                    color: self.current_color.clone(),
                };
            }
        }

        self.cursor_col += char_width;
        self.mark_render_dirty();
    }

    pub fn newline(&mut self) {
        self.clear_arrow_key_protection();
        self.cursor_col = 0;
        self.cursor_row += 1;

        // Always add new line to main_buffer when cursor moves to new row
        while self.cursor_row >= self.main_buffer.len() {
            self.main_buffer
                .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
        }

        // History management: trim old lines if exceeds maximum
        while self.main_buffer.len() > MAX_HISTORY_LINES {
            self.main_buffer.pop_front();
            // Adjust cursor_row if it's affected by the removal
            if self.cursor_row > 0 {
                self.cursor_row -= 1;
            }
        }

        self.mark_render_dirty();
    }

    pub fn carriage_return(&mut self) {
        self.cursor_col = 0;
        self.mark_render_dirty();
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            // This is a simplified backspace. A more correct implementation
            // would handle wide characters properly.
            self.cursor_col -= 1;
            if self.cursor_row < self.main_buffer.len() {
                let buffer = &mut self.main_buffer[self.cursor_row];
                if self.cursor_col < buffer.len() {
                    buffer[self.cursor_col] = TerminalCell::default();
                }
            }
        }
        self.mark_render_dirty();
    }

    pub fn move_cursor_to(&mut self, row: usize, col: usize) {
        if self.is_alt_screen {
            // In alt screen mode, limit to screen bounds
            self.cursor_row = row.min(self.rows - 1);
            self.cursor_col = col.min(self.cols - 1);
        } else {
            // In main screen mode, limit to buffer bounds
            self.cursor_row = row.min(self.main_buffer.len() - 1);
            self.cursor_col = col.min(MAX_MAIN_BUFFER_COLS - 1);
        }
        self.mark_render_dirty();
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
            // Save current main buffer state
            self.main_buffer_backup = Some(self.main_buffer.clone());
            self.saved_cursor_main = (self.cursor_row, self.cursor_col);

            // Switch to alternative screen - initialize main_buffer as clean screen
            self.main_buffer.clear();
            // Create initial rows to match screen size
            for _ in 0..self.rows {
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
            }
            self.is_alt_screen = true;
            self.cursor_row = 0;
            self.cursor_col = 0;

            println!("🔄 Switched to alternative screen buffer (using main_buffer)");
            self.mark_render_dirty();
        }
    }

    // Switch back to main screen buffer
    pub fn switch_to_main_screen(&mut self) {
        if self.is_alt_screen {
            // Don't save alt screen state - each app gets a clean alt screen
            // Just restore main screen
            if let Some(backup) = self.main_buffer_backup.take() {
                self.main_buffer = backup;
            }
            self.cursor_row = self.saved_cursor_main.0;
            self.cursor_col = self.saved_cursor_main.1;
            self.is_alt_screen = false;

            println!("🔄 Restored main screen buffer");
            self.mark_render_dirty();
        }
    }
}
