use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

pub const MAX_HISTORY_LINES: usize = 10;
pub const MAX_MAIN_BUFFER_COLS: usize = 1000; // Fixed width for main_buffer to preserve original data

// ANSI 색상 정보를 저장하는 구조체
#[derive(Clone, Copy, Debug, PartialEq)]
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
#[derive(Clone, Copy, Debug, PartialEq)]
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
    pub incremental_update: bool, // true = only process changed rows, false = full reflow

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

    // Scrolling region (DECSTBM)
    pub scroll_region_top: usize, // Top line of scrolling region (0-based)
    pub scroll_region_bottom: usize, // Bottom line of scrolling region (0-based)

    // Viewport tracking for optimized render_buffer updates
    pub visible_start_row: usize, // First visible row in render_buffer
    pub visible_end_row: usize,   // Last visible row in render_buffer (exclusive)

    // Render update throttling to reduce frequent updates during fast data input
    pub last_render_update_time: Option<Instant>,
    pub render_update_interval_ms: u64, // Minimum interval between updates (milliseconds)
}

impl TerminalState {
    // Find the actual end of text in a row (excluding trailing spaces)
    fn find_row_text_end(&self, row: &Vec<TerminalCell>) -> usize {
        row.iter()
            .rposition(|cell| cell.ch != ' ' && cell.ch != '\u{0000}')
            .map_or(0, |i| i + 1)
    }

    // Fast bulk copy for terminal cells using unsafe operations
    unsafe fn fast_copy_cells(&self, src: &[TerminalCell], dst: &mut [TerminalCell], count: usize) {
        debug_assert!(count <= src.len());
        debug_assert!(count <= dst.len());
        debug_assert!(count > 0);
        std::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), count);
    }

    // Safe bulk copy wrapper with optional unsafe optimization
    fn bulk_copy_cells(&self, src: &[TerminalCell], dst: &mut [TerminalCell]) -> usize {
        let copy_len = src.len().min(dst.len());
        if copy_len > 0 {
            // Use optimized copy strategy based on size
            if copy_len >= 100 && copy_len <= 1000 {
                // Sweet spot for unsafe optimization
                unsafe {
                    self.fast_copy_cells(src, dst, copy_len);
                }
            } else {
                // Use safe copy for very small or very large chunks
                dst[..copy_len].copy_from_slice(&src[..copy_len]);
            }
        }
        copy_len
    }

    // Alternative safe bulk copy (for comparison)
    fn safe_bulk_copy_cells(&self, src: &[TerminalCell], dst: &mut [TerminalCell]) -> usize {
        let copy_len = src.len().min(dst.len());
        if copy_len > 0 {
            dst[..copy_len].copy_from_slice(&src[..copy_len]);
        }
        copy_len
    }

    // Performance test method to compare different copy approaches
    #[allow(dead_code)]
    fn benchmark_copy_methods(&self) {
        use std::time::Instant;

        // Test multiple sizes to see where unsafe wins
        let test_sizes = [10, 50, 100, 500, 1000, 5000];
        let iterations = 50000;

        println!(
            "📊 Copy Performance Benchmark ({} iterations per test):",
            iterations
        );
        println!("Size\t\tSafe(μs)\tUnsafe(μs)\tHybrid(μs)\tUnsafe Speedup\tHybrid Speedup");
        println!(
            "─────────────────────────────────────────────────────────────────────────────────"
        );

        for &size in &test_sizes {
            // Create test data
            let test_row = vec![TerminalCell::default(); size];
            let mut dest_row = vec![TerminalCell::default(); size];

            // Test 1: Safe copy_from_slice
            let start = Instant::now();
            for _ in 0..iterations {
                dest_row[..size].copy_from_slice(&test_row[..size]);
            }
            let safe_duration = start.elapsed();

            // Test 2: Unsafe ptr::copy_nonoverlapping
            let start = Instant::now();
            for _ in 0..iterations {
                unsafe {
                    self.fast_copy_cells(&test_row, &mut dest_row, size);
                }
            }
            let unsafe_duration = start.elapsed();

            // Test 3: bulk_copy_cells (hybrid approach)
            let start = Instant::now();
            for _ in 0..iterations {
                self.bulk_copy_cells(&test_row[..size], &mut dest_row[..size]);
            }
            let hybrid_duration = start.elapsed();

            let safe_us = safe_duration.as_nanos() as f64 / 1000.0;
            let unsafe_us = unsafe_duration.as_nanos() as f64 / 1000.0;
            let hybrid_us = hybrid_duration.as_nanos() as f64 / 1000.0;

            let unsafe_speedup = safe_us / unsafe_us;
            let hybrid_speedup = safe_us / hybrid_us;

            println!(
                "{:>4}\t\t{:>7.1}\t\t{:>9.1}\t{:>9.1}\t{:>11.2}x\t{:>12.2}x",
                size, safe_us, unsafe_us, hybrid_us, unsafe_speedup, hybrid_speedup
            );
        }

        println!(
            "─────────────────────────────────────────────────────────────────────────────────"
        );
        if cfg!(debug_assertions) {
            println!("Note: Results in DEBUG mode. Run with --release for optimized performance.");
        } else {
            println!("Note: Results in RELEASE mode with full optimization.");
        }
    }

    // Mark render_buffer as dirty for batch update
    pub fn mark_render_dirty(&mut self) {
        self.render_buffer_dirty = true;
    }

    // Update render_buffer from main_buffer's visible area (only if dirty and throttled)
    pub fn update_render_buffer_if_dirty(&mut self) {
        if !self.render_buffer_dirty {
            return;
        }

        // Check throttling: only update if enough time has passed
        let now = Instant::now();
        if let Some(last_update) = self.last_render_update_time {
            let elapsed = now.duration_since(last_update).as_millis() as u64;
            if elapsed < self.render_update_interval_ms {
                return; // Skip update, not enough time has passed
            }
        }

        self.update_render_buffer();
        self.render_buffer_dirty = false;
        self.last_render_update_time = Some(now);
    }

    // Force update render_buffer regardless of throttling (for important operations)
    pub fn force_update_render_buffer(&mut self) {
        self.update_render_buffer();
        self.render_buffer_dirty = false;
        self.last_render_update_time = Some(Instant::now());
    }

    // Update viewport information for optimized rendering
    pub fn update_viewport(&mut self, visible_start: usize, visible_end: usize) {
        self.visible_start_row = visible_start;
        self.visible_end_row = visible_end;
    }

    // Set render update interval for throttling control
    pub fn set_render_update_interval(&mut self, interval_ms: u64) {
        self.render_update_interval_ms = interval_ms;
    }

    // Get current render update interval
    pub fn get_render_update_interval(&self) -> u64 {
        self.render_update_interval_ms
    }

    // Update render buffer and calculate cursor offset in one pass (like commit 7a65ed9)
    pub fn update_render_buffer(&mut self) {
        // Clear render_buffer first
        self.render_buffer.clear();

        // Calculate viewport range with safety margin for reflow
        let viewport_margin = self.rows * 2; // Extra rows before/after viewport
        let safe_start = self.visible_start_row.saturating_sub(viewport_margin);
        let safe_end = (self.visible_end_row + viewport_margin).min(self.main_buffer.len());

        // Smart incremental vs full reflow based on operation type
        let (process_start, process_end) = if self.incremental_update {
            // PTY data: no reflow needed, just copy main_buffer as-is to render_buffer
            // Shell/PTY already handles line wrapping and formatting
            // Simply copy all rows without reflow processing
            self.copy_main_to_render_without_reflow();
            return; // Skip reflow processing entirely
        } else {
            // Full reflow: process entire buffer (resize, clear, etc.)
            (0, self.main_buffer.len())
        };

        let mut main_buffer_idx = process_start;

        // Process selected range of main_buffer rows and reflow them into render_buffer
        while main_buffer_idx < process_end {
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
                    self.bulk_copy_cells(
                        &source_row[..copy_length],
                        &mut render_row[..copy_length],
                    );
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

                        // Optimization: Try to bulk copy consecutive normal-width characters
                        let start_source_col = source_col;
                        let start_render_col = render_col;
                        let mut consecutive_normal_chars = 0;

                        // Count consecutive normal-width characters
                        while render_col < self.cols && source_col < text_end {
                            if source_row[source_col].ch == '\u{0000}' {
                                break;
                            }
                            let char_width = source_row[source_col].ch.width().unwrap_or(1);
                            if char_width != 1 || render_col + 1 > self.cols {
                                break;
                            }
                            consecutive_normal_chars += 1;
                            render_col += 1;
                            source_col += 1;
                        }

                        // Bulk copy if we found consecutive normal characters
                        if consecutive_normal_chars > 0 {
                            self.bulk_copy_cells(
                                &source_row
                                    [start_source_col..start_source_col + consecutive_normal_chars],
                                &mut render_row
                                    [start_render_col..start_render_col + consecutive_normal_chars],
                            );

                            // Check cursor position in the bulk copied range
                            if is_cursor_row
                                && self.cursor_col >= start_source_col
                                && self.cursor_col < start_source_col + consecutive_normal_chars
                            {
                                self.render_cursor_row = self.render_buffer.len();
                                self.render_cursor_col =
                                    start_render_col + (self.cursor_col - start_source_col);
                            }
                        }

                        // Handle remaining character (wide character or end condition)
                        if render_col < self.cols
                            && source_col < text_end
                            && source_row[source_col].ch != '\u{0000}'
                        {
                            let char_width = source_row[source_col].ch.width().unwrap_or(1);
                            if render_col + char_width <= self.cols {
                                render_row[render_col] = source_row[source_col];

                                // For wide characters, mark the second cell as continuation
                                if char_width == 2 && render_col + 1 < self.cols {
                                    render_row[render_col + 1] = TerminalCell {
                                        ch: '\u{0000}',
                                        color: source_row[source_col].color,
                                    };
                                }

                                // Check if this is where cursor should be (for cursor row)
                                if is_cursor_row && source_col == self.cursor_col {
                                    self.render_cursor_row = self.render_buffer.len();
                                    self.render_cursor_col = render_col;
                                }

                                render_col += char_width;
                                source_col += 1;
                            } else {
                                break; // Can't fit this character
                            }
                        }
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

    // Fast copy for PTY data without reflow (shell already handles wrapping)
    fn copy_main_to_render_without_reflow(&mut self) {
        // Clear render_buffer first
        self.render_buffer.clear();

        // Simply copy each main_buffer row to render_buffer without reflow
        for (row_idx, source_row) in self.main_buffer.iter().enumerate() {
            let is_cursor_row = row_idx == self.cursor_row;

            // Find actual text end in this row
            let text_end = self.find_row_text_end(source_row);

            // Create render row with terminal width
            let mut render_row = vec![TerminalCell::default(); self.cols];
            let copy_length = text_end.min(self.cols);

            // Copy content up to terminal width or text end
            if copy_length > 0 {
                self.bulk_copy_cells(&source_row[..copy_length], &mut render_row[..copy_length]);
            }

            self.render_buffer.push(render_row);

            // Track cursor position in render buffer
            if is_cursor_row {
                self.render_cursor_row = self.render_buffer.len() - 1;
                self.render_cursor_col = self.cursor_col.min(self.cols);
            }
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
            incremental_update: false, // Start with full reflow
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
            scroll_region_top: 0,
            scroll_region_bottom: rows - 1,
            visible_start_row: 0,
            visible_end_row: rows, // Initially show first 'rows' lines
            last_render_update_time: None,
            render_update_interval_ms: 33, // ~60 FPS (16ms interval)
        };
        state.update_render_buffer();

        // Run performance benchmark when explicitly requested
        if std::env::var("WTERM_BENCHMARK").is_ok() {
            println!("🚀 Running copy performance benchmark...");
            state.benchmark_copy_methods();
        }

        state
    }

    pub fn clear_screen(&mut self) {
        self.main_buffer.clear();
        self.main_buffer
            .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.incremental_update = false; // Full reflow required for clear
        self.force_update_render_buffer(); // Clear screen needs immediate update
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
        self.incremental_update = false; // Full reflow required for resize
        self.force_update_render_buffer(); // Resize needs immediate update

        // Ensure cursor is within new bounds
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));

        // Ensure main_buffer has at least one row
        if self.main_buffer.is_empty() {
            self.main_buffer
                .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
        }

        self.cursor_row = self.cursor_row.min(self.main_buffer.len() - 1);

        // Update scroll region to match new terminal size
        self.scroll_region_top = 0;
        self.scroll_region_bottom = new_rows - 1;

        // Update viewport to match new terminal size
        self.visible_end_row = new_rows;
    }

    pub fn put_char(&mut self, ch: char) {
        // Skip frequent arrow key protection clearing for performance
        // self.clear_arrow_key_protection();
        let char_width = ch.width().unwrap_or(1);

        // Ensure row exists in main_buffer
        while self.cursor_row >= self.main_buffer.len() {
            self.main_buffer
                .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
        }

        // Skip frequent auto-scroll checks for performance - let newline handle it
        // self.auto_scroll_if_needed();

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
            color: self.current_color,
        };

        if char_width == 2 {
            if self.cursor_col + 1 < buffer.len() {
                buffer[self.cursor_col + 1] = TerminalCell {
                    ch: '\u{0000}', // Continuation marker
                    color: self.current_color,
                };
            }
        }

        self.cursor_col += char_width;
        self.incremental_update = true; // Only current row needs reflow
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

        // Only auto-scroll every 10 lines for better performance during heavy output
        if self.cursor_row % 10 == 0 {
            self.auto_scroll_if_needed();
        }

        // History management: trim old lines if exceeds maximum
        while self.main_buffer.len() > MAX_HISTORY_LINES {
            self.main_buffer.pop_front();
            // Adjust cursor_row if it's affected by the removal
            if self.cursor_row > 0 {
                self.cursor_row -= 1;
            }
        }

        self.incremental_update = true; // Only affected rows need reflow
        self.mark_render_dirty();
    }

    pub fn carriage_return(&mut self) {
        self.cursor_col = 0;
        self.mark_render_dirty();
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            // Move cursor back to find the character to delete
            let mut delete_col = self.cursor_col - 1;

            // If we're on a continuation marker (\u{0000}), move back to the actual character
            if self.cursor_row < self.main_buffer.len() {
                let row = &self.main_buffer[self.cursor_row];
                while delete_col > 0 && delete_col < row.len() && row[delete_col].ch == '\u{0000}' {
                    delete_col -= 1;
                }
            }

            if self.cursor_row < self.main_buffer.len() {
                let row = &mut self.main_buffer[self.cursor_row];
                if delete_col < row.len() {
                    // Get the character we're about to delete
                    let ch_to_delete = row[delete_col].ch;
                    let char_width = ch_to_delete.width().unwrap_or(1);

                    // Clear the character and any continuation markers
                    for i in 0..char_width {
                        if delete_col + i < row.len() {
                            row[delete_col + i] = TerminalCell::default();
                        }
                    }

                    // Move cursor to the position of the deleted character
                    self.cursor_col = delete_col;
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

    // Set scrolling region (DECSTBM - DEC Set Top and Bottom Margins)
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        // Convert from 1-based to 0-based indexing
        let top = if top > 0 { top - 1 } else { 0 };
        let bottom = if bottom > 0 {
            bottom - 1
        } else {
            self.rows - 1
        };

        // Validate bounds
        if top < self.rows && bottom < self.rows && top <= bottom {
            self.scroll_region_top = top;
            self.scroll_region_bottom = bottom;

            // Move cursor to top-left of scrolling region (as per VT100 spec)
            self.cursor_row = self.main_buffer.len().saturating_sub(self.rows) + top;
            self.cursor_col = 0;

            println!("📜 Set scroll region: top={}, bottom={}", top, bottom);
        } else {
            // Reset to full screen if invalid parameters
            self.scroll_region_top = 0;
            self.scroll_region_bottom = self.rows - 1;
            self.cursor_row = self.main_buffer.len().saturating_sub(self.rows);
            self.cursor_col = 0;

            println!("📜 Reset scroll region to full screen");
        }
        self.mark_render_dirty();
    }

    // Scroll up within the scrolling region (SU - Scroll Up)
    pub fn scroll_up_in_region(&mut self, lines: usize) {
        let lines = if lines == 0 { 1 } else { lines };

        for _ in 0..lines {
            // Calculate absolute positions in main_buffer
            let buffer_offset = self.main_buffer.len().saturating_sub(self.rows);
            let top_abs = buffer_offset + self.scroll_region_top;
            let bottom_abs = buffer_offset + self.scroll_region_bottom;

            // Ensure we have enough buffer space
            while self.main_buffer.len() <= bottom_abs {
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
            }

            // Remove the top line of the scrolling region
            if top_abs < self.main_buffer.len() {
                self.main_buffer.remove(top_abs);
            }

            // Add a new blank line at the bottom of the scrolling region
            let new_line = vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS];
            if bottom_abs < self.main_buffer.len() {
                self.main_buffer.insert(bottom_abs, new_line);
            } else {
                self.main_buffer.push_back(new_line);
            }
        }

        println!("📜 Scrolled up {} lines in region", lines);
        self.mark_render_dirty();
    }

    // Scroll down within the scrolling region (SD - Scroll Down)
    pub fn scroll_down_in_region(&mut self, lines: usize) {
        let lines = if lines == 0 { 1 } else { lines };

        for _ in 0..lines {
            // Calculate absolute positions in main_buffer
            let buffer_offset = self.main_buffer.len().saturating_sub(self.rows);
            let top_abs = buffer_offset + self.scroll_region_top;
            let bottom_abs = buffer_offset + self.scroll_region_bottom;

            // Ensure we have enough buffer space
            while self.main_buffer.len() <= bottom_abs {
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
            }

            // Remove the bottom line of the scrolling region
            if bottom_abs < self.main_buffer.len() {
                self.main_buffer.remove(bottom_abs);
            }

            // Add a new blank line at the top of the scrolling region
            let new_line = vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS];
            self.main_buffer.insert(top_abs, new_line);
        }

        println!("📜 Scrolled down {} lines in region", lines);
        self.mark_render_dirty();
    }

    // Auto-scroll to keep cursor in visible area
    pub fn auto_scroll_if_needed(&mut self) {
        if self.is_alt_screen {
            // In alt-screen mode (vi, less, etc.), use scrolling region
            let screen_start = self.main_buffer.len().saturating_sub(self.rows);
            let cursor_screen_row = self.cursor_row.saturating_sub(screen_start);

            // If cursor is beyond the bottom of the scrolling region, scroll up
            if cursor_screen_row > self.scroll_region_bottom {
                let lines_to_scroll = cursor_screen_row - self.scroll_region_bottom;
                for _ in 0..lines_to_scroll {
                    self.scroll_up_in_region(1);
                }
                // Move cursor back to bottom of scrolling region
                self.cursor_row = screen_start + self.scroll_region_bottom;
                println!(
                    "🔄 Alt-screen auto-scroll: scrolled {} lines, cursor at screen row {}",
                    lines_to_scroll, self.scroll_region_bottom
                );
            }
        } else {
            // In normal mode, simply ensure buffer grows as needed
            // The render system will automatically show the bottom part of the buffer
            // Debug output disabled for performance during large outputs
            // println!(
            //     "🔄 Normal mode: cursor at row {}, buffer size {}",
            //     self.cursor_row,
            //     self.main_buffer.len()
            // );
        }
    }

    // IND (Index) - Move cursor down one line, scroll if at bottom of scrolling region
    pub fn index_down(&mut self) {
        self.clear_arrow_key_protection();

        if self.is_alt_screen {
            // In alt-screen mode, check if at bottom of scrolling region
            let screen_start = self.main_buffer.len().saturating_sub(self.rows);
            let cursor_screen_row = self.cursor_row.saturating_sub(screen_start);

            if cursor_screen_row >= self.scroll_region_bottom {
                // At bottom of scrolling region - scroll up
                self.scroll_up_in_region(1);
                println!("🔄 IND: Scrolled up in region, cursor stays at bottom");
            } else {
                // Move cursor down normally
                self.cursor_row += 1;
                while self.cursor_row >= self.main_buffer.len() {
                    self.main_buffer
                        .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
                }
                println!("🔄 IND: Moved cursor down to row {}", cursor_screen_row + 1);
            }
        } else {
            // In normal mode, just move cursor down and grow buffer as needed
            self.cursor_row += 1;
            while self.cursor_row >= self.main_buffer.len() {
                self.main_buffer
                    .push_back(vec![TerminalCell::default(); MAX_MAIN_BUFFER_COLS]);
            }
            // println!(
            //     "🔄 IND: Normal mode, moved cursor to row {}",
            //     self.cursor_row
            // );
        }

        self.mark_render_dirty();
    }

    // NEL (Next Line) - Move to beginning of next line, scroll if at bottom
    pub fn next_line(&mut self) {
        self.index_down(); // Move down one line with scroll
        self.cursor_col = 0; // Move to beginning of line
        println!("🔄 NEL: Moved to beginning of next line");
    }

    // RI (Reverse Index) - Move cursor up one line, scroll if at top of scrolling region
    pub fn reverse_index(&mut self) {
        self.clear_arrow_key_protection();

        if self.is_alt_screen {
            // In alt-screen mode, check if at top of scrolling region
            let screen_start = self.main_buffer.len().saturating_sub(self.rows);
            let cursor_screen_row = self.cursor_row.saturating_sub(screen_start);

            if cursor_screen_row <= self.scroll_region_top {
                // At top of scrolling region - scroll down
                self.scroll_down_in_region(1);
                println!("🔄 RI: Scrolled down in region, cursor stays at top");
            } else {
                // Move cursor up normally
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                }
                println!("🔄 RI: Moved cursor up to row {}", cursor_screen_row - 1);
            }
        } else {
            // In normal mode, just move cursor up
            if self.cursor_row > 0 {
                self.cursor_row -= 1;
            }
            // println!(
            //     "🔄 RI: Normal mode, moved cursor to row {}",
            //     self.cursor_row
            // );
        }

        self.mark_render_dirty();
    }
}
