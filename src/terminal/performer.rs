use crate::terminal::state::{AnsiColor, TerminalCell, TerminalState};
use crate::utils::color::ansi_256_to_rgb;
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vte::{Params, Perform};

// VTE Performer implementation
pub struct TerminalPerformer {
    state: Arc<Mutex<TerminalState>>,
    egui_ctx: egui::Context,
    last_repaint_time: Instant,
    repaint_interval: Duration,
    initial_repaints: u32, // Track initial repaints to skip throttling
}

impl TerminalPerformer {
    pub fn new(state: Arc<Mutex<TerminalState>>, egui_ctx: egui::Context) -> Self {
        Self {
            state,
            egui_ctx,
            last_repaint_time: Instant::now(),
            repaint_interval: Duration::from_millis(8), // ~120fps limit for more responsive updates
            initial_repaints: 0,                        // Start counting initial repaints
        }
    }

    // Request repaint only if enough time has passed (throttled)
    fn request_repaint_throttled(&mut self) {
        // Skip throttling for the first many repaints to ensure immediate initial rendering
        if self.initial_repaints < 50 {
            self.egui_ctx.request_repaint();
            self.initial_repaints += 1;
            self.last_repaint_time = Instant::now();
            return;
        }

        let now = Instant::now();
        if now.duration_since(self.last_repaint_time) >= self.repaint_interval {
            self.egui_ctx.request_repaint();
            self.last_repaint_time = now;
        }
    }

    // Request immediate repaint for important terminal events
    fn request_repaint_immediate(&mut self) {
        self.egui_ctx.request_repaint();
        self.last_repaint_time = Instant::now();
    }
}

impl Perform for TerminalPerformer {
    fn print(&mut self, c: char) {
        if let Ok(mut state) = self.state.lock() {
            // Don't filter leading spaces - let them through normally
            // The PROMPT_EOL_MARK="" setting should handle the root cause

            state.put_char(c);
        } // Drop state lock before repaint

        // Use throttled repaint for better performance
        self.request_repaint_throttled();
    }

    fn execute(&mut self, byte: u8) {
        let (state_changed, needs_immediate_repaint) = if let Ok(mut state) = self.state.lock() {
            let mut changed = false;
            let mut immediate = false;

            match byte {
                b'\n' => {
                    state.newline();
                    changed = true;
                    immediate = true; // Important event - repaint immediately
                }
                b'\r' => {
                    // Process carriage return but don't trigger newline
                    state.carriage_return();
                    changed = true;
                    immediate = true; // Important event - repaint immediately
                }
                b'\x08' => {
                    if !state.should_protect_from_arrow_key() {
                        state.backspace();
                        changed = true;
                    }
                }
                b'\x09' => {
                    let next_tab_stop = ((state.cursor_col / 8) + 1) * 8;
                    if next_tab_stop < state.cols {
                        state.cursor_col = next_tab_stop;
                    } else {
                        state.cursor_col = state.cols - 1;
                    }
                    changed = true;
                }
                b'\x0c' => {
                    state.clear_arrow_key_protection();
                    state.clear_screen();
                    changed = true;
                    immediate = true; // Clear screen - repaint immediately
                }
                b'\x7f' => {
                    if !state.should_protect_from_arrow_key() {
                        state.backspace();
                        changed = true;
                    }
                }
                _ => {}
            }

            (changed, immediate)
        } else {
            (false, false)
        }; // Drop state lock before repaint

        if state_changed {
            if needs_immediate_repaint {
                self.request_repaint_immediate();
            } else {
                self.request_repaint_throttled();
            }
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _c: char) {
        // No-op
    }

    fn put(&mut self, _byte: u8) {
        // No-op
    }

    fn unhook(&mut self) {
        // No-op
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        println!(
            "🖥️ DEBUG: VTE osc_dispatch - bell_terminated: {}, params: {:?}",
            bell_terminated,
            params
                .iter()
                .map(|p| String::from_utf8_lossy(p))
                .collect::<Vec<_>>()
        );
        // No-op
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, c: char) {
        let state_changed = if let Ok(mut state) = self.state.lock() {
            let mut state_changed = false;

            match c {
                'H' | 'f' => {
                    // CUP (Cursor Position) or HVP (Horizontal and Vertical Position)
                    let row = params.iter().next().unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    let col = params.iter().nth(1).unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    state.move_cursor_to(row, col);
                    state_changed = true;
                }
                'J' => {
                    // ED (Erase in Display)
                    let param = params.iter().next().unwrap_or(&[0])[0];
                    match param {
                        0 => { // Clear from cursor to end of screen
                            if state.is_alt_screen {
                                let cursor_row = state.cursor_row;
                                let cursor_col = state.cursor_col;
                                let rows = state.rows;
                                let cols = state.cols;
                                for row_idx in cursor_row..rows {
                                    let start_col = if row_idx == cursor_row { cursor_col } else { 0 };
                                    for col_idx in start_col..cols {
                                        if row_idx < state.alt_screen.len() && col_idx < state.alt_screen[row_idx].len() {
                                            state.alt_screen[row_idx][col_idx] = TerminalCell::default();
                                        }
                                    }
                                }
                            } else {
                                let cursor_row = state.cursor_row;
                                let cursor_col = state.cursor_col;
                                if cursor_row < state.main_buffer.len() {
                                    let row = &mut state.main_buffer[cursor_row];
                                    for col_idx in cursor_col..row.len() {
                                        row[col_idx] = TerminalCell::default();
                                    }
                                }
                                let buffer_len = state.main_buffer.len();
                                for row_idx in (cursor_row + 1)..buffer_len {
                                    state.main_buffer[row_idx].fill(TerminalCell::default());
                                }
                            }
                            state.mark_render_dirty();
                            state_changed = true;
                        }
                        1 => { // Clear from start of screen to cursor
                            if state.is_alt_screen {
                                let cursor_row = state.cursor_row;
                                let cursor_col = state.cursor_col;
                                let cols = state.cols;
                                for row_idx in 0..=cursor_row {
                                    let end_col = if row_idx == cursor_row { cursor_col + 1 } else { cols };
                                    for col_idx in 0..end_col {
                                        if row_idx < state.alt_screen.len() && col_idx < state.alt_screen[row_idx].len() {
                                            state.alt_screen[row_idx][col_idx] = TerminalCell::default();
                                        }
                                    }
                                }
                            } else {
                                let cursor_row = state.cursor_row;
                                for row_idx in 0..cursor_row {
                                    if row_idx < state.main_buffer.len() {
                                        state.main_buffer[row_idx].fill(TerminalCell::default());
                                    }
                                }
                                let cursor_col = state.cursor_col;
                                if cursor_row < state.main_buffer.len() {
                                    let row = &mut state.main_buffer[cursor_row];
                                    for col_idx in 0..=cursor_col {
                                        if col_idx < row.len() {
                                            row[col_idx] = TerminalCell::default();
                                        }
                                    }
                                }
                            }
                            state.mark_render_dirty();
                            state_changed = true;
                        }
                        2 => { // Clear entire screen
                            if state.is_alt_screen {
                                for row in state.alt_screen.iter_mut() {
                                    row.fill(TerminalCell::default());
                                }
                            } else {
                                let cols = state.cols;
                                state.main_buffer.clear();
                                state.main_buffer.push_back(vec![TerminalCell::default(); cols]);
                            }
                            state.move_cursor_to(0, 0);
                            state.mark_render_dirty();
                            state_changed = true;
                        }
                        3 => { // Clear entire screen and scrollback buffer
                            if state.is_alt_screen {
                                for row in state.alt_screen.iter_mut() {
                                    row.fill(TerminalCell::default());
                                }
                            } else {
                                let cols = state.cols;
                                state.main_buffer.clear();
                                state.main_buffer.push_back(vec![TerminalCell::default(); cols]);
                            }
                            state.move_cursor_to(0, 0);
                            state.mark_render_dirty();
                            state_changed = true;
                        }
                        _ => {}
                    }
                }
                'K' => {
                    // EL (Erase in Line)
                    let param = params.iter().next().unwrap_or(&[0])[0];
                    let cursor_row = state.cursor_row;
                    let cursor_col = state.cursor_col;

                    let line = if state.is_alt_screen {
                        &mut state.alt_screen[cursor_row]
                    } else {
                        &mut state.main_buffer[cursor_row]
                    };

                    match param {
                        0 => { // Clear from cursor to end of line
                            for col in cursor_col..line.len() {
                                line[col] = TerminalCell::default();
                            }
                        }
                        1 => { // Clear from start of line to cursor
                            for col in 0..=cursor_col {
                                if col < line.len() {
                                    line[col] = TerminalCell::default();
                                }
                            }
                        }
                        2 => { // Clear entire line
                            line.fill(TerminalCell::default());
                        }
                        _ => {}
                    }
                    state.mark_render_dirty();
                    state_changed = true;
                }
                'A' => {
                    // CUU (Cursor Up) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    let count = if count == 0 { 1 } else { count }; // ANSI standard: 0 means 1
                    state.cursor_row = state.cursor_row.saturating_sub(count);
                    state.set_arrow_key_protection();
                    state_changed = true;
                }
                'B' => {
                    // CUD (Cursor Down) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    let rows = if state.is_alt_screen { state.rows } else { state.main_buffer.len() };
                    state.cursor_row = (state.cursor_row + count).min(rows - 1);
                    state.set_arrow_key_protection();
                    state_changed = true;
                }
                'C' => {
                    // CUF (Cursor Forward) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    let cols = state.cols;
                    state.cursor_col = (state.cursor_col + count).min(cols - 1);
                    state.set_arrow_key_protection();
                    state_changed = true;
                }
                'D' => {
                    // CUB (Cursor Backward) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_col = state.cursor_col.saturating_sub(count);
                    state.set_arrow_key_protection();
                    state_changed = true;
                }
                'm' => {
                    // SGR (Select Graphic Rendition) - colors and text attributes
                    if params.is_empty() {
                        // Reset to defaults
                        state.current_color = AnsiColor::default();
                    } else {
                        // Process SGR parameters sequentially, handling multi-parameter sequences
                        let param_vec: Vec<_> = params.iter().collect();
                        let mut i = 0;
                        while i < param_vec.len() {
                            if let Some(&code) = param_vec[i].first() {
                                match code {
                                    0 => state.current_color = AnsiColor::default(), // Reset
                                    1 => state.current_color.bold = true,            // Bold
                                    3 => state.current_color.italic = true,          // Italic
                                    4 => state.current_color.underline = true,       // Underline
                                    7 => state.current_color.reverse = true, // Reverse video
                                    22 => state.current_color.bold = false,  // Normal intensity
                                    23 => state.current_color.italic = false, // Not italic
                                    24 => state.current_color.underline = false, // Not underlined
                                    27 => state.current_color.reverse = false, // Not reversed
                                    // Foreground colors (8-color) - macOS Terminal compatible
                                    30 => state.current_color.foreground = ansi_256_to_rgb(0), // Black
                                    31 => state.current_color.foreground = ansi_256_to_rgb(1), // Red
                                    32 => state.current_color.foreground = ansi_256_to_rgb(2), // Green
                                    33 => state.current_color.foreground = ansi_256_to_rgb(3), // Yellow
                                    34 => state.current_color.foreground = ansi_256_to_rgb(4), // Blue
                                    35 => state.current_color.foreground = ansi_256_to_rgb(5), // Magenta
                                    36 => state.current_color.foreground = ansi_256_to_rgb(6), // Cyan
                                    37 => state.current_color.foreground = ansi_256_to_rgb(7), // White
                                    // Bright foreground colors
                                    90 => state.current_color.foreground = ansi_256_to_rgb(8), // Bright Black
                                    91 => state.current_color.foreground = ansi_256_to_rgb(9), // Bright Red
                                    92 => state.current_color.foreground = ansi_256_to_rgb(10), // Bright Green
                                    93 => state.current_color.foreground = ansi_256_to_rgb(11), // Bright Yellow
                                    94 => state.current_color.foreground = ansi_256_to_rgb(12), // Bright Blue
                                    95 => state.current_color.foreground = ansi_256_to_rgb(13), // Bright Magenta
                                    96 => state.current_color.foreground = ansi_256_to_rgb(14), // Bright Cyan
                                    97 => state.current_color.foreground = ansi_256_to_rgb(15), // Bright White
                                    // Background colors (40-47)
                                    40 => state.current_color.background = ansi_256_to_rgb(0), // Black
                                    41 => state.current_color.background = ansi_256_to_rgb(1), // Red
                                    42 => state.current_color.background = ansi_256_to_rgb(2), // Green
                                    43 => state.current_color.background = ansi_256_to_rgb(3), // Yellow
                                    44 => state.current_color.background = ansi_256_to_rgb(4), // Blue
                                    45 => state.current_color.background = ansi_256_to_rgb(5), // Magenta
                                    46 => state.current_color.background = ansi_256_to_rgb(6), // Cyan
                                    47 => state.current_color.background = ansi_256_to_rgb(7), // White
                                    // Bright background colors (100-107)
                                    100 => state.current_color.background = ansi_256_to_rgb(8), // Bright Black
                                    101 => state.current_color.background = ansi_256_to_rgb(9), // Bright Red
                                    102 => state.current_color.background = ansi_256_to_rgb(10), // Bright Green
                                    103 => state.current_color.background = ansi_256_to_rgb(11), // Bright Yellow
                                    104 => state.current_color.background = ansi_256_to_rgb(12), // Bright Blue
                                    105 => state.current_color.background = ansi_256_to_rgb(13), // Bright Magenta
                                    106 => state.current_color.background = ansi_256_to_rgb(14), // Bright Cyan
                                    107 => state.current_color.background = ansi_256_to_rgb(15), // Bright White
                                    // Default colors
                                    39 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(203, 204, 205)
                                    } // Default foreground
                                    49 => {
                                        state.current_color.background = egui::Color32::TRANSPARENT
                                    } // Default background
                                    // Extended color sequences
                                    38 => {
                                        // Foreground color: 38;5;n or 38;2;r;g;b
                                        if i + 2 < param_vec.len() {
                                            if let Some(&subtype) = param_vec[i + 1].first() {
                                                if subtype == 5 && i + 2 < param_vec.len() {
                                                    // 256-color: ESC[38;5;nm
                                                    if let Some(&color_idx) =
                                                        param_vec[i + 2].first()
                                                    {
                                                        state.current_color.foreground =
                                                            ansi_256_to_rgb(color_idx as u8);
                                                        i += 2; // Skip the next 2 parameters
                                                    }
                                                } else if subtype == 2 && i + 4 < param_vec.len() {
                                                    // RGB: ESC[38;2;r;g;bm
                                                    if let (Some(&r), Some(&g), Some(&b)) = (
                                                        param_vec[i + 2].first(),
                                                        param_vec[i + 3].first(),
                                                        param_vec[i + 4].first(),
                                                    ) {
                                                        state.current_color.foreground =
                                                            egui::Color32::from_rgb(
                                                                r as u8, g as u8, b as u8,
                                                            );
                                                        i += 4; // Skip the next 4 parameters
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    48 => {
                                        // Background color: 48;5;n or 48;2;r;g;b
                                        if i + 2 < param_vec.len() {
                                            if let Some(&subtype) = param_vec[i + 1].first() {
                                                if subtype == 5 && i + 2 < param_vec.len() {
                                                    // 256-color: ESC[48;5;nm
                                                    if let Some(&color_idx) =
                                                        param_vec[i + 2].first()
                                                    {
                                                        state.current_color.background =
                                                            ansi_256_to_rgb(color_idx as u8);
                                                        i += 2; // Skip the next 2 parameters
                                                    }
                                                } else if subtype == 2 && i + 4 < param_vec.len() {
                                                    // RGB: ESC[48;2;r;g;bm
                                                    if let (Some(&r), Some(&g), Some(&b)) = (
                                                        param_vec[i + 2].first(),
                                                        param_vec[i + 3].first(),
                                                        param_vec[i + 4].first(),
                                                    ) {
                                                        state.current_color.background =
                                                            egui::Color32::from_rgb(
                                                                r as u8, g as u8, b as u8,
                                                            );
                                                        i += 4; // Skip the next 4 parameters
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {
                                        // Unknown SGR code - ignore
                                    }
                                }
                            }
                            i += 1;
                        }
                    }
                    state_changed = true; // Colors changed
                }
                'h' | 'l' => {
                    // Set Mode (h) / Reset Mode (l) - often used for terminal features
                    let is_private_mode = intermediates.contains(&b'?');

                    if let Some(first_param) = params.iter().next() {
                        let mode = first_param[0];

                        if is_private_mode {
                            // Private mode sequences (ESC[?...h/l)
                            match mode {
                                1 => {
                                    // Application cursor keys mode - silently ignore
                                }
                                25 => {
                                    // Cursor visibility mode
                                    if c == 'h' {
                                        state.cursor_visible = true;
                                    } else {
                                        state.cursor_visible = false;
                                    }
                                    state_changed = true;
                                }
                                1049 => {
                                    // Alternative screen buffer
                                    if c == 'h' {
                                        // ESC[?1049h - Switch to alternative screen buffer
                                        state.switch_to_alt_screen();
                                    } else {
                                        // ESC[?1049l - Switch back to main screen buffer
                                        state.switch_to_main_screen();
                                    }
                                    state_changed = true;
                                }
                                _ => {
                                    // Silently ignore other private modes
                                }
                            }
                        } else {
                            // Standard mode sequences (ESC[...h/l)
                            match mode {
                                2004 => {
                                    // Bracketed paste mode - silently ignore
                                }
                                _ => {
                                    // Silently ignore other standard modes
                                }
                            }
                        }
                    }
                }
                'd' => {
                    // VPA (Vertical Position Absolute)
                    let row = params.iter().next().unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    let rows = if state.is_alt_screen { state.rows } else { state.main_buffer.len() };
                    state.cursor_row = row.min(rows - 1);
                    state_changed = true;
                }
                'G' => {
                    // CHA (Cursor Horizontal Absolute)
                    let col = params.iter().next().unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    let cols = state.cols;
                    state.cursor_col = col.min(cols - 1);
                    state_changed = true;
                }
                't' => {
                    // Window manipulation sequences - ignore
                }
                'n' => {
                    // Device Status Report - ignore
                }
                'c' => {
                    // Device Attributes - ignore
                }
                'r' => {
                    // Set scrolling region - ignore for now
                }
                'S' => {
                    // Scroll up - ignore for now
                }
                'T' => {
                    // Scroll down - ignore for now
                }
                'X' => {
                    // ECH (Erase Character) - Erase N characters from cursor position
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    let cursor_col = state.cursor_col;
                    let cols = state.cols;
                    let row_idx = state.cursor_row;

                    let buffer_len = if state.is_alt_screen {
                        state.alt_screen.len()
                    } else {
                        state.main_buffer.len()
                    };

                    if row_idx < buffer_len {
                        let line = if state.is_alt_screen {
                            &mut state.alt_screen[row_idx]
                        } else {
                            &mut state.main_buffer[row_idx]
                        };

                        for i in 0..count {
                            if cursor_col + i < cols {
                                if (cursor_col + i) < line.len() {
                                    line[cursor_col + i] = TerminalCell::default();
                                }
                            }
                        }
                    }
                    state.mark_render_dirty();
                    state_changed = true;
                }
                'P' => {
                    // DCH (Delete Character) - COMPLETELY BLOCKED
                }
                '@' => {
                    // ICH (Insert Character) - ignore for now
                }
                'L' => {
                    // Insert line - ignore for now
                }
                'M' => {
                    // Delete line - ignore for now
                }
                's' => {
                    // Save cursor position (ANSI.SYS compatible)
                    println!(
                        "💾 CSI s: Saving cursor ({}, {})",
                        state.cursor_row, state.cursor_col
                    );
                    if state.is_alt_screen {
                        state.saved_cursor_alt = (state.cursor_row, state.cursor_col);
                    } else {
                        state.saved_cursor_main = (state.cursor_row, state.cursor_col);
                    }
                    state_changed = true;
                }
                'u' => {
                    // Restore cursor position (ANSI.SYS compatible)
                    let (row, col) = if state.is_alt_screen {
                        state.saved_cursor_alt
                    } else {
                        state.saved_cursor_main
                    };
                    println!("🔄 CSI u: Restoring cursor to ({}, {})", row, col);
                    state.move_cursor_to(row, col);
                    state_changed = true;
                }
                _ => {
                    // Silently ignore unknown CSI sequences
                    // This helps with compatibility with complex prompts
                }
            }

            state_changed
        } else {
            false
        }; // Drop state lock before repaint

        // Signal repaint if state changed
        if state_changed {
            self.request_repaint_throttled();
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        let state_changed = if let Ok(mut state) = self.state.lock() {
            let mut changed = false;
            match byte {
                b'7' => {
                    // Save Cursor (DECSC)
                    if state.is_alt_screen {
                        state.saved_cursor_alt = (state.cursor_row, state.cursor_col);
                    } else {
                        state.saved_cursor_main = (state.cursor_row, state.cursor_col);
                    }
                    changed = true;
                }
                b'8' => {
                    // Restore Cursor (DECRC)
                    let (row, col) = if state.is_alt_screen {
                        state.saved_cursor_alt
                    } else {
                        state.saved_cursor_main
                    };
                    state.move_cursor_to(row, col);
                    changed = true;
                }
                _ => {}
            }

            changed
        } else {
            false
        }; // Drop state lock before repaint

        if state_changed {
            self.request_repaint_throttled();
        }
    }
}
