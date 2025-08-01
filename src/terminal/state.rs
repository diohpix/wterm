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
    // Main buffer: all history + current screen in one VecDeque
    pub main_buffer: VecDeque<Vec<TerminalCell>>,
    pub visible_start: usize, // Start of currently visible area

    // Render buffer: current screen content for efficient rendering
    pub render_buffer: Vec<Vec<TerminalCell>>,
    pub render_buffer_dirty: bool, // Flag to track if render_buffer needs update

    pub cursor_row: usize, // Logical position relative to visible area
    pub cursor_col: usize,
    pub cursor_offset_row: usize, // Additional rows due to reflow
    pub rows: usize,              // Visible rows
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
    // Get the actual render position of cursor
    pub fn get_render_cursor_row(&self) -> usize {
        if self.is_alt_screen {
            self.cursor_row // Alt screen doesn't use offset
        } else {
            (self.cursor_row + self.cursor_offset_row).min(self.rows.saturating_sub(1))
        }
    }

    // Find the actual end of text in a row (excluding trailing spaces)
    fn find_row_text_end(&self, row: &Vec<TerminalCell>) -> usize {
        // Find the last non-space, non-null character
        for i in (0..row.len()).rev() {
            if row[i].ch != ' ' && row[i].ch != '\u{0000}' {
                return i + 1; // Return length (index + 1)
            }
        }
        0 // Empty row
    }

    // Mark render_buffer as dirty for batch update
    pub fn mark_render_dirty(&mut self) {
        self.render_buffer_dirty = true;
    }

    // Update render_buffer from main_buffer's visible area (only if dirty)
    pub fn update_render_buffer_if_dirty(&mut self) {
        if self.render_buffer_dirty {
            self.update_render_buffer(); // Now calculates cursor offset internally
            self.render_buffer_dirty = false;
        }
    }

    // Update render buffer and calculate cursor offset in one pass
    pub fn update_render_buffer(&mut self) {
        // Clear render_buffer first
        for row in &mut self.render_buffer {
            row.fill(TerminalCell::default());
        }

        let mut render_row_idx = 0;
        let mut main_buffer_idx = self.visible_start;

        // Calculate cursor absolute position for offset calculation
        let cursor_absolute_row = if self.is_alt_screen {
            // Alt screen doesn't use offset
            self.cursor_offset_row = 0;
            usize::MAX // Use invalid value to skip offset calculation
        } else {
            self.visible_start + self.cursor_row
        };

        let mut cursor_render_row = None; // Will store the render row where cursor should be

        // Process main_buffer rows and reflow them into render_buffer
        while render_row_idx < self.rows && main_buffer_idx < self.main_buffer.len() {
            let source_row = &self.main_buffer[main_buffer_idx];
            let is_cursor_row = main_buffer_idx == cursor_absolute_row;

            // Find the actual end of text in this row
            let text_end = self.find_row_text_end(source_row);

            // Debug: print row information for analysis
            if main_buffer_idx >= self.visible_start && main_buffer_idx < self.visible_start + 3 {
                let row_text: String = source_row[0..text_end.min(50)]
                    .iter()
                    .map(|cell| if cell.ch == '\u{0000}' { ' ' } else { cell.ch })
                    .collect();
                println!(
                    "🔍 Row {}: text_end={}, cols={}, needs_reflow={}, text='{}'",
                    main_buffer_idx,
                    text_end,
                    self.cols,
                    text_end > self.cols,
                    row_text
                );
            }

            // Check if this row needs reflow based on actual text length
            let needs_reflow = text_end > self.cols;

            if !needs_reflow {
                // Simple copy without reflow - only copy up to text end or cols
                let copy_length = text_end.min(self.cols);
                if copy_length > 0 {
                    self.render_buffer[render_row_idx][..copy_length]
                        .clone_from_slice(&source_row[..copy_length]);
                }

                // If this is cursor row, record the render row
                if is_cursor_row {
                    cursor_render_row = Some(render_row_idx);
                }

                render_row_idx += 1;
            } else {
                // Reflow: split long row across multiple render rows
                let mut source_col = 0;
                let cursor_render_start = render_row_idx; // Remember where this row starts

                while source_col < text_end && render_row_idx < self.rows {
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

                        // Copy character to render_buffer
                        self.render_buffer[render_row_idx][render_col] =
                            source_row[source_col].clone();

                        // For wide characters, mark continuation
                        if char_width == 2 && render_col + 1 < self.cols {
                            self.render_buffer[render_row_idx][render_col + 1] = TerminalCell {
                                ch: '\u{0000}',
                                color: source_row[source_col].color.clone(),
                            };
                        }

                        // Check if this is where cursor should be (for cursor row)
                        if is_cursor_row
                            && cursor_render_row.is_none()
                            && source_col == self.cursor_col
                        {
                            cursor_render_row = Some(render_row_idx);
                        }

                        render_col += char_width;
                        source_col += 1;
                    }

                    // Move to next render row
                    render_row_idx += 1;
                }

                // If cursor was in this row but not found yet (at end of line or beyond),
                // place it at the last render row for this main_buffer row
                if is_cursor_row && cursor_render_row.is_none() {
                    cursor_render_row = Some((render_row_idx - 1).max(cursor_render_start));
                }
            }

            // Always move to next main_buffer row after processing
            main_buffer_idx += 1;
        }

        // Calculate cursor offset based on the render row we found
        if !self.is_alt_screen {
            if let Some(render_row) = cursor_render_row {
                self.cursor_offset_row = render_row.saturating_sub(self.cursor_row);
            } else {
                self.cursor_offset_row = 0; // Fallback if cursor not found
            }
        }

        self.render_buffer_dirty = false;
    }

    pub fn new(rows: usize, cols: usize) -> Self {
        let mut main_buffer = VecDeque::with_capacity(MAX_HISTORY_LINES + rows);

        // Start with just one empty row - more will be added as needed
        main_buffer.push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);

        let render_buffer = vec![vec![TerminalCell::default(); cols]; rows];
        let alt_screen = vec![vec![TerminalCell::default(); cols]; rows];

        let mut state = Self {
            main_buffer,
            visible_start: 0,
            render_buffer,
            render_buffer_dirty: false,
            cursor_row: 0,
            cursor_col: 0,
            cursor_offset_row: 0, // Initialize offset to 0
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
        };

        // Initialize render_buffer with main_buffer content
        state.update_render_buffer(); // This now calculates cursor offset internally
                                      // Mark as dirty to ensure initial render
        state.render_buffer_dirty = true;
        state
    }

    // Note: ensure_at_bottom removed to prevent interference with resize logic
    // visible_start is now managed entirely by resize() method

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

        // Update render_buffer after clear
        self.update_render_buffer();
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }

        // Calculate current cursor absolute position BEFORE changing anything
        let old_cursor_absolute_row = if self.is_alt_screen {
            self.cursor_row
        } else {
            self.visible_start + self.cursor_row
        };

        self.rows = new_rows;
        self.cols = new_cols;

        if self.is_alt_screen {
            // For alt screen, just recreate with new dimensions
            self.alt_screen = vec![vec![TerminalCell::default(); new_cols]; new_rows];
            self.cursor_offset_row = 0; // Reset offset for alt screen
        } else {
            // For main screen, adjust visible_start
            if self.main_buffer.len() >= new_rows {
                self.visible_start = self.main_buffer.len() - new_rows;
            } else {
                self.visible_start = 0;
            }
        }

        // Adjust cursor position
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));

        // Adjust cursor_row based on the new visible_start (for main screen only)
        if !self.is_alt_screen {
            if old_cursor_absolute_row >= self.visible_start {
                self.cursor_row = old_cursor_absolute_row - self.visible_start;
            } else {
                self.cursor_row = 0;
            }
            self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        } else {
            self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        }

        // Update saved cursor positions
        self.saved_cursor_main.0 = self.saved_cursor_main.0.min(new_rows.saturating_sub(1));
        self.saved_cursor_main.1 = self.saved_cursor_main.1.min(new_cols.saturating_sub(1));
        self.saved_cursor_alt = (0, 0);

        // Update render_buffer with new dimensions
        self.render_buffer = vec![vec![TerminalCell::default(); new_cols]; new_rows];

        // Update render buffer (which now calculates cursor offset internally)
        self.update_render_buffer();
    }

    pub fn put_char(&mut self, ch: char) {
        // Reset arrow key state when adding new text
        self.clear_arrow_key_protection();

        // Get the display width of the character
        let char_width = ch.width().unwrap_or(1);

        // Check if we have enough space for this character
        if self.is_alt_screen && self.cursor_col + char_width > self.cols {
            self.newline();
        }

        if self.cursor_row < self.rows {
            let absolute_row = if self.is_alt_screen {
                self.cursor_row
            } else {
                self.visible_start + self.cursor_row
            };

            // Ensure we have enough rows in main_buffer
            while absolute_row >= self.main_buffer.len() {
                println!("🔤 ADDING ROW: {}", absolute_row);
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
            }

            // Ensure the row has enough columns (expand to MAX_MAIN_BUFFER_COLS if needed)
            if self.main_buffer[absolute_row].len() < MAX_MAIN_BUFFER_COLS {
                self.main_buffer[absolute_row]
                    .resize(MAX_MAIN_BUFFER_COLS, TerminalCell::default());
            }

            // Place the character with current color (with bounds check)
            if self.cursor_col < MAX_MAIN_BUFFER_COLS
                && self.cursor_col < self.main_buffer[absolute_row].len()
            {
                self.main_buffer[absolute_row][self.cursor_col] = TerminalCell {
                    ch,
                    color: self.current_color.clone(),
                };
            }

            // For wide characters (width 2), mark the second cell as a continuation
            if char_width == 2
                && self.cursor_col + 1 < MAX_MAIN_BUFFER_COLS
                && self.cursor_col + 1 < self.main_buffer[absolute_row].len()
            {
                self.main_buffer[absolute_row][self.cursor_col + 1] = TerminalCell {
                    ch: '\u{0000}', // Null char as continuation marker
                    color: self.current_color.clone(),
                };
            }

            // Move cursor by the character width
            self.cursor_col += char_width;
        }

        // If we've reached the end of the line, wrap to next line
        if self.is_alt_screen && self.cursor_col >= self.cols {
            self.newline();
        }

        // Mark render_buffer as dirty for batch update later
        if !self.is_alt_screen {
            self.mark_render_dirty();
        }
    }

    pub fn newline(&mut self) {
        // Reset arrow key state when moving to new line
        self.clear_arrow_key_protection();

        self.cursor_row += 1;
        self.cursor_col = 0;
        if self.cursor_row >= self.rows {
            if self.is_alt_screen {
                // Alt screen doesn't scroll - just stay at bottom
                self.cursor_row = self.rows - 1;
            } else {
                // Main screen: add new line and scroll
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);

                // Trim if exceeds maximum history
                while self.main_buffer.len() > MAX_HISTORY_LINES {
                    self.main_buffer.pop_front();
                    // Adjust visible_start when removing from front
                    if self.visible_start > 0 {
                        self.visible_start -= 1;
                    }
                }

                // Update visible_start to show the bottom of the buffer (scroll down)
                if self.main_buffer.len() >= self.rows {
                    self.visible_start = self.main_buffer.len() - self.rows;
                } else {
                    self.visible_start = 0;
                }

                self.cursor_row = self.rows - 1;
            }
        }

        // Mark render_buffer as dirty for batch update later
        if !self.is_alt_screen {
            self.mark_render_dirty();
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

        // Mark render_buffer as dirty for batch update later
        if !self.is_alt_screen {
            self.mark_render_dirty();
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
            self.cursor_row = self.saved_cursor_main.0.min(self.rows.saturating_sub(1));
            self.cursor_col = self.saved_cursor_main.1.min(self.cols.saturating_sub(1));

            println!("🔄 Restored main screen buffer");
        }
    }
}
