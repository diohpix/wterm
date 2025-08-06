use anyhow::Result;
use eframe::egui;
use portable_pty::{CommandBuilder, PtySize};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;
use vte::Parser;

use crate::ime::korean::KoreanInputState;
use crate::terminal::performer::TerminalPerformer;
use crate::terminal::state::TerminalState;

// Main terminal application
pub struct TerminalApp {
    terminal_state: Arc<Mutex<TerminalState>>,
    pty_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pty_master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    korean_state: KoreanInputState,
    last_tab_time: Option<Instant>, // Tab key debouncing
    initial_focus_set: bool,        // Flag to track if initial focus has been set
}

impl TerminalApp {
    // Process text input with Korean composition support
    fn process_text_input(&mut self, text: &str) {
        // Reset arrow key state when text is being input
        if let Ok(mut state) = self.terminal_state.lock() {
            state.clear_arrow_key_protection();
        }

        for ch in text.chars() {
            self.process_single_char(ch);
        }
    }

    // Process a single character with Korean composition logic
    fn process_single_char(&mut self, ch: char) {
        if crate::ime::korean::is_consonant(ch) || crate::ime::korean::is_vowel(ch) {
            // Handle Korean input - only send completed characters to PTY
            if let Some(completed) = self.process_korean_char(ch) {
                self.send_to_pty(&completed.to_string());
            }
            // Composing characters are only shown visually, not sent to PTY
        } else {
            // Non-Korean character - finish any pending composition and send the character
            self.finalize_korean_composition();
            self.send_to_pty(&ch.to_string());
        }
    }

    // Process Korean character input and return completed character if any
    fn process_korean_char(&mut self, ch: char) -> Option<char> {
        // println!("🔤 Processing Korean char: '{}' (U+{:04X})", ch, ch as u32); // Disabled for performance
        if crate::ime::korean::is_consonant(ch) {
            if self.korean_state.chosung.is_none() {
                // First consonant - set as chosung, start composing
                self.korean_state.chosung = Some(ch);
                self.korean_state.is_composing = true;
                return None; // Don't send anything to PTY yet
            } else if self.korean_state.jungsung.is_some() && self.korean_state.jongsung.is_none() {
                // We have chosung + jungsung, this consonant becomes jongsung
                self.korean_state.jongsung = Some(ch);
                return None; // Still composing
            } else if let Some(existing_jong) = self.korean_state.jongsung {
                // Try to combine with existing jongsung
                if let Some(combined) = crate::ime::korean::combine_consonants(existing_jong, ch) {
                    self.korean_state.jongsung = Some(combined);
                    return None; // Still composing
                } else {
                    // Can't combine - complete current syllable and start new one
                    let completed = self.korean_state.get_current_char();
                    // if let Some(c) = completed {
                    //     println!("✅ Completing syllable (consonant can't combine): '{}'", c);
                    // }
                    self.korean_state.reset();
                    self.korean_state.chosung = Some(ch);
                    self.korean_state.is_composing = true;
                    return completed; // Send completed character
                }
            } else {
                // Already have chosung but no jungsung - complete current and start new
                let completed = self.korean_state.get_current_char();
                if let Some(c) = completed {
                    println!("✅ Completing syllable (no jungsung): '{}'", c);
                }
                self.korean_state.reset();
                self.korean_state.chosung = Some(ch);
                self.korean_state.is_composing = true;
                return completed; // Send completed character
            }
        } else if crate::ime::korean::is_vowel(ch) {
            if self.korean_state.chosung.is_some() && self.korean_state.jungsung.is_none() {
                // We have chosung, this vowel becomes jungsung
                self.korean_state.jungsung = Some(ch);
                return None; // Still composing
            } else if let Some(existing_jung) = self.korean_state.jungsung {
                // Check if we have jongsung - if so, we need to move it to new syllable
                if let Some(jong) = self.korean_state.jongsung {
                    // Complete current syllable without the jongsung (ㄱㅏㄴ->ㄱㅏ완성, ㄴㅏ시작)
                    let cho_idx =
                        crate::ime::korean::get_chosung_index(self.korean_state.chosung.unwrap())
                            .unwrap();
                    let jung_idx = crate::ime::korean::get_jungsung_index(existing_jung).unwrap();
                    let completed = crate::ime::korean::compose_korean(cho_idx, jung_idx, 0); // No jongsung

                    // Start new syllable with jongsung as chosung
                    self.korean_state.reset();
                    self.korean_state.chosung = Some(jong);
                    self.korean_state.jungsung = Some(ch);
                    self.korean_state.is_composing = true;
                    println!(
                        "✅ Completing syllable (vowel with jongsung split): '{}'",
                        completed
                    );
                    return Some(completed); // Send completed "가", keep "나" composing
                } else {
                    // Try to combine with existing jungsung
                    if let Some(combined) = crate::ime::korean::combine_vowels(existing_jung, ch) {
                        self.korean_state.jungsung = Some(combined);
                        return None; // Still composing
                    } else {
                        // Can't combine - complete current syllable
                        let completed = self.korean_state.get_current_char();
                        // if let Some(c) = completed {
                        //     println!("✅ Completing syllable (vowel can't combine): '{}'", c);
                        // }
                        self.korean_state.reset();
                        // Vowel can't start a new syllable without consonant, so just send it
                        return completed;
                    }
                }
            } else {
                // No chosung yet - vowel can't start syllable, just send it
                return Some(ch);
            }
        }

        None
    }

    // Finalize any pending Korean composition
    fn finalize_korean_composition(&mut self) {
        if self.korean_state.is_composing {
            if let Some(completed) = self.korean_state.get_current_char() {
                // println!("🏁 Finalizing Korean composition: '{}'", completed); // Disabled for performance
                self.send_to_pty(&completed.to_string());
            }
            self.korean_state.reset();
        }
    }

    // Helper function to send text to PTY
    fn send_to_pty(&mut self, text: &str) {
        // println!(
        //     "📤 DEBUG: Sending to PTY: {:?} (bytes: {:?})",
        //     text,
        //     text.as_bytes()
        // ); // Disabled for performance

        if let Ok(mut writer) = self.pty_writer.lock() {
            let _ = writer.write_all(text.as_bytes());
            let _ = writer.flush();
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        // Configure custom font with better fallback
        let mut fonts = egui::FontDefinitions::default();

        // Load D2Coding font from file
        let d2coding_font_data = include_bytes!("../assets/fonts/D2Coding.ttf");
        fonts.font_data.insert(
            "D2Coding".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(d2coding_font_data)),
        );

        // Set D2Coding as the primary monospace font, but keep existing fallbacks
        let monospace_fonts = fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap();
        monospace_fonts.insert(0, "D2Coding".to_owned());

        // Also add D2Coding to proportional for UI text
        let proportional_fonts = fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap();
        proportional_fonts.insert(0, "D2Coding".to_owned());

        cc.egui_ctx.set_fonts(fonts);

        // Calculate a reasonable *initial* terminal size based on estimates.
        // This will be corrected on the first frame in `update()`.
        let (actual_rows, actual_cols, initial_pixel_width, initial_pixel_height) = {
            let line_height = 16.0f32; // Estimate
            let char_width = 7.5f32; // Estimate, adjusted for better fit

            // Use default window size from main() for initial calculation
            let available_height = 768.0f32;
            let available_width = 1024.0f32;

            // Leave some margin for UI elements and window chrome
            let usable_height = available_height - 100.0;
            let usable_width = available_width - 50.0;

            let rows = (usable_height / line_height).floor() as usize;
            let cols = (usable_width / char_width).floor() as usize;

            let rows = rows.max(20).min(100);
            let cols = cols.max(60).min(200);

            let pixel_width = (cols as f32 * char_width) as u16;
            let pixel_height = (rows as f32 * line_height) as u16;
            (rows, cols, pixel_width, pixel_height)
        };

        println!(
            "🖥️ Initial estimated terminal size: {}x{} ({}x{}px)",
            actual_cols, actual_rows, initial_pixel_width, initial_pixel_height
        );

        // Use calculated size
        let terminal_state = Arc::new(Mutex::new(TerminalState::new(actual_rows, actual_cols)));

        // Create PTY with calculated size, including pixel dimensions for accuracy
        let pty_system = portable_pty::native_pty_system();
        let pty_pair = pty_system.openpty(PtySize {
            rows: actual_rows as u16,
            cols: actual_cols as u16,
            pixel_width: initial_pixel_width,
            pixel_height: initial_pixel_height,
        })?;

        // Spawn shell - use zsh with user configs (.zshrc, oh-my-zsh etc)
        let mut cmd = CommandBuilder::new("/bin/zsh");
        cmd.args(&["-il"]); // Login shell with user's .zshrc
        cmd.env("TERM", "xterm-256color");
        cmd.env("LANG", "ko_KR.UTF-8");
        cmd.env("LC_ALL", "ko_KR.UTF-8");
        cmd.env("LC_CTYPE", "UTF-8");
        cmd.env("SHELL", "/bin/zsh");
        //P1: '\\x1b]0;', P2: '\\x07'
        cmd.env("PROMPT_EOL_MARK", "%{%G%}");
        // Ensure consistent terminal behavior and fix visual glitches
        cmd.env("TERM_PROGRAM", "wterm");
        cmd.env("TERM_PROGRAM_VERSION", "1.0");
        // Disable the reverse-video '%' character at the end of partial lines

        // Prevent oh-my-zsh from trying to set the window title
        cmd.env("DISABLE_AUTO_TITLE", "true");

        let _child = pty_pair.slave.spawn_command(cmd)?;

        let mut pty_reader = pty_pair.master.try_clone_reader()?;
        let pty_writer = Arc::new(Mutex::new(pty_pair.master.take_writer()?));
        let pty_master = Arc::new(Mutex::new(pty_pair.master));

        // Spawn background thread to read from PTY
        let state_clone = terminal_state.clone();
        let egui_ctx_clone = cc.egui_ctx.clone();
        thread::spawn(move || {
            let mut parser = Parser::new();
            let mut performer = TerminalPerformer::new(state_clone, egui_ctx_clone);

            let mut buffer = [0u8; 1024];
            loop {
                match pty_reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let read_data = &buffer[..n];

                        /*    println!(
                            "🚽 PTY Read ({} bytes): string: \"{}\"",
                            n,
                            String::from_utf8_lossy(read_data).escape_debug()
                        );*/

                        // Process all bytes at once using VTE 0.15 API
                        parser.advance(&mut performer, read_data);
                    }
                    Err(_) => break,
                }
            }
        });

        // Request initial repaint to ensure first render
        cc.egui_ctx.request_repaint();

        Ok(Self {
            terminal_state,
            pty_writer,
            pty_master,
            korean_state: KoreanInputState::new(),
            last_tab_time: None,
            initial_focus_set: false,
        })
    }

    fn calculate_terminal_size(
        &self,
        available_rect: egui::Rect,
        ui: &egui::Ui,
    ) -> (usize, usize, u16, u16) {
        let font_id = egui::FontId::new(11.0, egui::FontFamily::Monospace);
        let line_height = ui.fonts(|f| f.row_height(&font_id));
        let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'M'));

        // Use most of the available space, leaving small margin for scrollbar
        let usable_height = available_rect.height() - 20.0; // Small margin for scrollbar
        let usable_width = available_rect.width() - 20.0; // Small margin for scrollbar

        let rows = (usable_height / line_height).floor() as usize;
        let cols = (usable_width / char_width).floor() as usize;

        // Minimum size constraints
        let rows = rows.max(10);
        let cols = cols.max(40);

        let pixel_width = (cols as f32 * char_width) as u16;
        let pixel_height = (rows as f32 * line_height) as u16;

        /*
                println!(
                    "🖥️ Dynamic terminal size: {}x{} ({}x{}px, rect: {}x{}, char: {}x{})",
                    cols,
                    rows,
                    pixel_width,
                    pixel_height,
                    available_rect.width(),
                    available_rect.height(),
                    char_width,
                    line_height
                );
        */

        (rows, cols, pixel_width, pixel_height)
    }

    fn resize_terminal(
        &mut self,
        new_rows: usize,
        new_cols: usize,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<()> {
        // Get current terminal size first
        let current_size = {
            let state = self.terminal_state.lock().unwrap();
            (state.rows, state.cols)
        };

        if (new_rows, new_cols) == current_size {
            return Ok(());
        }

        // Resize the terminal state
        {
            let mut state: std::sync::MutexGuard<'_, TerminalState> =
                self.terminal_state.lock().unwrap();
            let is_alt = state.is_alt_screen;
            state.resize(new_rows, new_cols);

            // Only clear alt screen, preserve main screen content
            if is_alt {
                state.clear_screen();
            }
        }

        // Resize the PTY and send SIGWINCH to notify shell of size change
        {
            let pty_master = self.pty_master.lock().unwrap();
            let new_size = PtySize {
                rows: new_rows as u16,
                cols: new_cols as u16,
                pixel_width,
                pixel_height,
            };
            //println!("🖥️ Resizing PTY to: {:?}", new_size);
            pty_master
                .resize(new_size)
                .map_err(|e| anyhow::anyhow!("PTY resize failed: {}", e))?;
        }

        Ok(())
    }
}

impl eframe::App for TerminalApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // No need to check IME timeout with rustkorean

        // We'll handle window rounding through the UI elements themselves

        // Check for drag anywhere in the window (simple approach)
        if ctx.input(|i| i.pointer.any_pressed()) && ctx.input(|i| i.pointer.primary_down()) {
            // Get the current pointer position
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                // If pointer is in the top area of the window (title bar area), start drag
                if pos.y < 50.0 {
                    // Top 50 pixels can be used for dragging
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            }
        }

        // Draw the entire window background with rounded corners
        let corner_radius_u8 = 10u8; // macOS-style corner radius

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT)) // Keep window background transparent
            .show(ctx, |ui| {
                // Draw the entire window background in one piece
                let full_rect = ui.available_rect_before_wrap();

                // Calculate title bar height
                let title_bar_height = 28.0;

                // Draw title bar background (top rounded corners)
                let title_rect = egui::Rect::from_min_size(
                    full_rect.min,
                    egui::Vec2::new(full_rect.width(), title_bar_height),
                );
                ui.painter().rect_filled(
                    title_rect,
                    egui::CornerRadius {
                        nw: corner_radius_u8,
                        ne: corner_radius_u8,
                        sw: 0,
                        se: 0,
                    },
                    egui::Color32::from_rgba_unmultiplied(60, 60, 60, 255), // Opaque title bar
                );

                // Draw terminal area background (bottom rounded corners)
                let terminal_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(full_rect.min.x, full_rect.min.y + title_bar_height),
                    egui::Vec2::new(full_rect.width(), full_rect.height() - title_bar_height),
                );
                ui.painter().rect_filled(
                    terminal_rect,
                    egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: corner_radius_u8,
                        se: corner_radius_u8,
                    },
                    egui::Color32::from_rgba_premultiplied(0, 0, 0, 178), // 70% opacity terminal
                );

                // Custom macOS-style title bar (just the content, background already drawn)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;

                    // Create title bar area (just for content positioning)
                    let (rect, _response) = ui.allocate_exact_size(
                        egui::Vec2::new(ui.available_width(), title_bar_height),
                        egui::Sense::hover(),
                    );

                    // macOS traffic light buttons (left side)
                    let button_size = 12.0;
                    let button_spacing = 20.0;
                    let left_margin = 12.0;
                    let button_y = rect.center().y;

                    // Close button (red)
                    let close_center = egui::Pos2::new(rect.left() + left_margin, button_y);
                    let close_rect =
                        egui::Rect::from_center_size(close_center, egui::Vec2::splat(button_size));
                    let close_response = ui.allocate_rect(close_rect, egui::Sense::click());

                    ui.painter().circle_filled(
                        close_center,
                        button_size / 2.0,
                        egui::Color32::from_rgb(255, 95, 87), // macOS red
                    );

                    if close_response.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }

                    // Minimize button (yellow)
                    let minimize_center =
                        egui::Pos2::new(rect.left() + left_margin + button_spacing, button_y);
                    let minimize_rect = egui::Rect::from_center_size(
                        minimize_center,
                        egui::Vec2::splat(button_size),
                    );
                    let minimize_response = ui.allocate_rect(minimize_rect, egui::Sense::click());

                    ui.painter().circle_filled(
                        minimize_center,
                        button_size / 2.0,
                        egui::Color32::from_rgb(255, 189, 46), // macOS yellow
                    );

                    if minimize_response.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }

                    // Maximize button (green)
                    let maximize_center =
                        egui::Pos2::new(rect.left() + left_margin + button_spacing * 2.0, button_y);
                    let maximize_rect = egui::Rect::from_center_size(
                        maximize_center,
                        egui::Vec2::splat(button_size),
                    );
                    let maximize_response = ui.allocate_rect(maximize_rect, egui::Sense::click());

                    ui.painter().circle_filled(
                        maximize_center,
                        button_size / 2.0,
                        egui::Color32::from_rgb(40, 201, 64), // macOS green
                    );

                    if maximize_response.clicked() {
                        // Toggle between maximized and normal
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                    }

                    // Draw title text (centered)
                    let title_text = "🖥️ WTerm: macOS 스타일 터미널";
                    let text_size = ui
                        .fonts(|f| {
                            f.layout_no_wrap(
                                title_text.to_string(),
                                egui::FontId::default(),
                                egui::Color32::WHITE,
                            )
                        })
                        .size();

                    let text_pos = egui::Pos2::new(
                        rect.center().x - text_size.x / 2.0,
                        rect.center().y - text_size.y / 2.0,
                    );

                    ui.painter().text(
                        text_pos,
                        egui::Align2::LEFT_TOP,
                        title_text,
                        egui::FontId::default(),
                        egui::Color32::WHITE,
                    );
                });

                ui.separator();

                // Background is already drawn above as one unified rounded rectangle

                // Calculate available space for terminal after header and info
                let remaining_rect = ui.available_rect_before_wrap();

                // Calculate terminal size based on the remaining space, including pixel dimensions
                let (terminal_rows, terminal_cols, pixel_width, pixel_height) =
                    self.calculate_terminal_size(remaining_rect, ui);

                // Resize terminal if needed
                self.resize_terminal(terminal_rows, terminal_cols, pixel_width, pixel_height)
                    .unwrap();

                // Terminal display with focus handling and proper scrolling
                let scroll_area = egui::ScrollArea::vertical()
                    .id_salt("terminal_scroll") // Use id_salt for persistent state (corrected from id_source)
                    .stick_to_bottom(true)
                    .auto_shrink([false; 2]);

                let terminal_response = scroll_area.show(ui, |ui| {
                    // Calculate exact font metrics
                    let font_id = egui::FontId::new(11.0, egui::FontFamily::Monospace);
                    let line_height = ui.fonts(|f| f.row_height(&font_id));
                    let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'M'));

                    if let Ok(mut state) = self.terminal_state.lock() {
                        // If the terminal state has changed, update the reflowed render buffer.
                        state.update_render_buffer_if_dirty();

                        let content_width = state.cols as f32 * char_width;
                        // The total height is now based on the reflowed render_buffer.
                        let total_lines = state.render_buffer.len();
                        let content_height = total_lines as f32 * line_height;

                        let (response, painter) = ui.allocate_painter(
                            egui::Vec2::new(content_width, content_height),
                            egui::Sense::click_and_drag()
                                .union(egui::Sense::focusable_noninteractive()),
                        );

                        // Background is already drawn above, no need to draw again here

                        if response.clicked() {
                            ui.memory_mut(|mem| mem.request_focus(response.id));
                        }

                        // --- Row Virtualization ---
                        let first_visible_row = ((ui.clip_rect().top() - response.rect.top())
                            / line_height)
                            .floor()
                            .max(0.0) as usize;
                        let last_visible_row = ((ui.clip_rect().bottom() - response.rect.top())
                            / line_height)
                            .ceil() as usize;
                        let last_visible_row = last_visible_row.min(total_lines);

                        // Update viewport information for optimized render_buffer updates
                        state.update_viewport(first_visible_row, last_visible_row);

                        // Draw only the visible rows from the render_buffer.
                        for row_idx in first_visible_row..last_visible_row {
                            // Safety check: ensure row_idx is within render_buffer bounds
                            if row_idx >= state.render_buffer.len() {
                                break;
                            }
                            let row_data = &state.render_buffer[row_idx];
                            let y = response.rect.top() + row_idx as f32 * line_height;
                            let mut col_offset = 0.0;

                            for cell in row_data.iter() {
                                if cell.ch == '\u{0000}' {
                                    continue;
                                }

                                let char_display_width = if cell.ch.width().unwrap_or(1) == 2 {
                                    2.0
                                } else {
                                    1.0
                                };
                                let display_width = char_display_width * char_width;
                                let x = response.rect.left() + col_offset;
                                let pos = egui::Pos2::new(x, y);
                                let cell_rect = egui::Rect::from_min_size(
                                    pos,
                                    egui::Vec2::new(display_width, line_height),
                                );

                                let (final_fg, final_bg) = if cell.color.reverse {
                                    (cell.color.background, cell.color.foreground)
                                } else {
                                    (cell.color.foreground, cell.color.background)
                                };

                                if final_bg != egui::Color32::TRANSPARENT {
                                    painter.rect_filled(
                                        cell_rect,
                                        egui::CornerRadius::ZERO,
                                        final_bg,
                                    );
                                }

                                if cell.ch != ' ' {
                                    let mut text_color = final_fg;
                                    if cell.color.bold {
                                        let [r, g, b, a] = text_color.to_array();
                                        text_color = egui::Color32::from_rgba_unmultiplied(
                                            (r as f32 * 1.3).min(255.0) as u8,
                                            (g as f32 * 1.3).min(255.0) as u8,
                                            (b as f32 * 1.3).min(255.0) as u8,
                                            a,
                                        );
                                    }
                                    painter.text(
                                        pos,
                                        egui::Align2::LEFT_TOP,
                                        cell.ch,
                                        font_id.clone(),
                                        text_color,
                                    );
                                    if cell.color.underline {
                                        let underline_y = y + line_height - 1.0;
                                        painter.line_segment(
                                            [
                                                egui::Pos2::new(x, underline_y),
                                                egui::Pos2::new(x + display_width, underline_y),
                                            ],
                                            egui::Stroke::new(1.0, text_color),
                                        );
                                    }
                                }
                                col_offset += display_width;
                            }
                        }

                        // Draw cursor based on the calculated visual position from TerminalState.
                        let cursor_y =
                            response.rect.top() + state.render_cursor_row as f32 * line_height;
                        if cursor_y >= ui.clip_rect().top()
                            && cursor_y + line_height <= ui.clip_rect().bottom()
                        {
                            let cursor_x =
                                response.rect.left() + state.render_cursor_col as f32 * char_width;

                            if self.korean_state.is_composing {
                                // println!("📍 Cursor position: row={}, col={}, x={}, y={}",
                                //     state.render_cursor_row, state.render_cursor_col, cursor_x, cursor_y);
                            }
                            if state.cursor_visible && !self.korean_state.is_composing {
                                let cursor_line_y = cursor_y + line_height - 2.0;
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::Pos2::new(cursor_x, cursor_line_y),
                                        egui::Vec2::new(char_width, 2.0),
                                    ),
                                    egui::CornerRadius::ZERO,
                                    egui::Color32::WHITE,
                                );
                            }
                            // Calculate cursor width for Korean composition if needed
                            let cursor_width = if self.korean_state.is_composing {
                                // Korean composing characters are always wide (2 chars)
                                2.0 * char_width
                            } else {
                                // Normal cursor width
                                char_width
                            };

                            // Draw composing character preview if Korean composition is active
                            if self.korean_state.is_composing {
                                if let Some(composing_char) = self.korean_state.get_current_char() {
                                    // Calculate precise cursor X position by walking through the row (like e32f82d)
                                    // This ensures accurate positioning regardless of render_buffer update timing
                                    let mut preview_x = response.rect.left();

                                    let cursor_row_data =
                                        if state.cursor_row < state.main_buffer.len() {
                                            Some(&state.main_buffer[state.cursor_row])
                                        } else {
                                            None
                                        };

                                    // Walk through the row to calculate precise cursor position
                                    if let Some(row) = cursor_row_data {
                                        for cell_idx in 0..state.cursor_col.min(row.len()) {
                                            let cell = &row[cell_idx];
                                            if cell.ch == '\u{0000}' {
                                                continue;
                                            }

                                            // Calculate display width like e32f82d
                                            let char_display_width =
                                                if cell.ch.width().unwrap_or(1) == 2 {
                                                    2 // Korean and other wide characters are 2 units
                                                } else {
                                                    1 // All other characters are 1 unit
                                                };
                                            preview_x += char_display_width as f32 * char_width;
                                        }
                                    }

                                    let preview_y = cursor_y;

                                    // println!("🎯 Composing preview at: cursor_col={}, calculated_x={}, y={} for char '{}' (using e32f82d method)",
                                    //     state.cursor_col, preview_x, preview_y, composing_char);

                                    // Draw composing character with a different color (gray/dimmed) to show it's temporary
                                    let preview_color = egui::Color32::from_rgb(150, 150, 150); // Gray preview color

                                    // Draw a subtle background to make the preview more visible
                                    let preview_bg =
                                        egui::Color32::from_rgba_unmultiplied(100, 100, 100, 50);
                                    painter.rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::Pos2::new(preview_x, preview_y),
                                            egui::Vec2::new(cursor_width, line_height),
                                        ),
                                        egui::CornerRadius::ZERO,
                                        preview_bg,
                                    );

                                    // Draw the composing character
                                    painter.text(
                                        egui::Pos2::new(preview_x, preview_y),
                                        egui::Align2::LEFT_TOP,
                                        composing_char,
                                        font_id.clone(),
                                        preview_color,
                                    );

                                    // Hide the normal cursor when composing
                                    // (The composing character serves as a visual cursor)
                                }
                            }
                        }

                        response
                    } else {
                        ui.allocate_response(egui::Vec2::new(800.0, 600.0), egui::Sense::click())
                    }
                });

                // Set initial focus when app starts
                if !self.initial_focus_set {
                    ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                    self.initial_focus_set = true;
                    println!("🎯 Initial focus set to terminal");
                }

                // Handle keyboard input when terminal has focus
                let has_focus = ui.memory(|mem| mem.has_focus(terminal_response.inner.id));

                // Handle Tab key with raw event processing and debouncing
                let tab_handled = ctx.input_mut(|i| {
                    let mut tab_press_found = false;

                    // Debug: Count total events and Tab events
                    let _total_events = i.events.len();

                    // Process all events and consume Tab events to prevent UI focus changes
                    i.events.retain(|event| {
                        match event {
                            egui::Event::Key {
                                key: egui::Key::Tab,
                                pressed: true,
                                ..
                            } => {
                                tab_press_found = true;
                                false // Always consume Tab events to prevent focus changes
                            }
                            egui::Event::Key {
                                key: egui::Key::Tab,
                                pressed: false,
                                ..
                            } => {
                                false // Also consume Tab release events
                            }
                            _ => true,
                        }
                    });

                    // Only handle Tab PRESS, ignore RELEASE to prevent duplicate sending
                    if tab_press_found {
                        true
                    } else {
                        false
                    }
                });

                // Send Tab to PTY with debouncing (only if enough time has passed since last Tab)
                if tab_handled {
                    let now = Instant::now();
                    let should_send = if let Some(last_time) = self.last_tab_time {
                        let elapsed = now.duration_since(last_time).as_millis();
                        elapsed > 100 // 100ms debounce (reduced from 200ms)
                    } else {
                        true // First Tab key
                    };

                    if should_send {
                        // Ensure terminal has focus before and after sending Tab
                        ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                        self.finalize_korean_composition();
                        self.send_to_pty("\t");
                        self.last_tab_time = Some(now);
                        // Force focus again after sending Tab to prevent losing focus
                        ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                    }
                }

                // Handle ESC key specially using direct input check
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    // Ensure terminal has focus
                    ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));

                    if self.korean_state.is_composing {
                        // 조합 중이면 조합만 완성하고 ESC는 무시
                        self.finalize_korean_composition();
                    } else {
                        // 조합 중이 아니면 정상적으로 ESC 처리
                        self.send_to_pty("\x1b");
                    }
                }

                // Check for Ctrl+I as Tab alternative (with debouncing)
                if ctx.input(|i| i.key_pressed(egui::Key::I) && i.modifiers.ctrl) {
                    let now = Instant::now();
                    let should_send = if let Some(last_time) = self.last_tab_time {
                        let elapsed = now.duration_since(last_time).as_millis();
                        elapsed > 100 // 100ms debounce (reduced from 200ms)
                    } else {
                        true // First Ctrl+I
                    };

                    if should_send {
                        // Ensure terminal has focus before and after sending Tab
                        ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                        self.finalize_korean_composition();
                        self.send_to_pty("\t");
                        self.last_tab_time = Some(now);
                        // Force focus again after sending Tab to prevent losing focus
                        ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                    }
                }

                if has_focus {
                    ctx.input(|i| {
                        // Debug: Log events only when relevant
                        let total_events = i.events.len();
                        if total_events > 0 && total_events < 3 {
                            //println!("🔍 DEBUG: Processing {} input events in key handler", total_events);
                        }

                        for event in &i.events {
                            match event {
                                egui::Event::Key {
                                    key,
                                    pressed,
                                    modifiers,
                                    ..
                                } => {
                                    // Skip Tab keys completely - they're handled above
                                    if *key == egui::Key::Tab {
                                        continue;
                                    }

                                    // Only process key PRESS events, ignore key RELEASE events
                                    if !pressed {
                                        continue;
                                    }

                                    // Debug: Log all other key events
                                    //println!("🔑 Key event: {:?} (modifiers: {:?})", key, modifiers);
                                    // Handle keys that should finalize Korean composition
                                    match key {
                                        egui::Key::Enter => {
                                            //println!("🔑 DEBUG: Enter key pressed");
                                            self.finalize_korean_composition();
                                            // Reset arrow key state when user presses Enter
                                            if let Ok(mut state) = self.terminal_state.lock() {
                                                state.clear_arrow_key_protection();
                                            }
                                            // Send newline instead of carriage return to avoid duplication
                                            self.send_to_pty("\n");
                                        }
                                        egui::Key::Space => {
                                            // Space is handled by Text event, don't handle it here
                                            // Don't finalize composition here - let Text event handle it
                                        }
                                        // Tab is handled above - no case needed here
                                        egui::Key::Backspace => {
                                            // Handle backspace for Korean composition
                                            if self.korean_state.is_composing {
                                                // Step-by-step Korean composition backspace (Korean IME only, no PTY)
                                                let _still_composing =
                                                    self.korean_state.handle_backspace();
                                                // Korean composition is purely local - don't send to PTY
                                            } else {
                                                // For regular backspace, let shell handle everything
                                                // Shell has its own prompt protection (readline, zle, etc.)
                                                if let Ok(mut state) = self.terminal_state.lock() {
                                                    state.clear_arrow_key_protection();
                                                }
                                                // Send backspace directly to shell - no terminal-level protection needed
                                                self.send_to_pty("\x08");
                                            }
                                        }
                                        egui::Key::ArrowUp => {
                                            if self.korean_state.is_composing {
                                                // 조합 중이면 조합만 완성하고 화살표는 무시
                                                self.finalize_korean_composition();
                                            } else {
                                                // Send to PTY for command history navigation
                                                self.send_to_pty("\x1b[A");
                                            }
                                        }
                                        egui::Key::ArrowDown => {
                                            if self.korean_state.is_composing {
                                                // 조합 중이면 조합만 완성하고 화살표는 무시
                                                self.finalize_korean_composition();
                                            } else {
                                                // Send to PTY for command history navigation
                                                self.send_to_pty("\x1b[B");
                                            }
                                        }
                                        egui::Key::ArrowRight => {
                                            if self.korean_state.is_composing {
                                                // 조합 중이면 조합만 완성하고 화살표는 무시
                                                self.finalize_korean_composition();
                                            } else {
                                                // DIRECT cursor movement - bypass PTY to avoid backspace issue
                                                if let Ok(mut state) = self.terminal_state.lock() {
                                                    state.set_arrow_key_protection();
                                                    let current_col = state.cursor_col;

                                                    // Find the user input area (after prompt)
                                                    let mut prompt_end = 0;
                                                    let mut text_end = 0;

                                                    // Use the visual row from the render_buffer for cursor movement logic
                                                    let row = if state.render_cursor_row
                                                        < state.render_buffer.len()
                                                    {
                                                        &state.render_buffer
                                                            [state.render_cursor_row]
                                                    } else {
                                                        continue;
                                                    };

                                                    if row.len() >= 2 {
                                                        for i in 0..(row.len() - 1) {
                                                            if (row[i].ch == '~'
                                                                || row[i].ch == '✗')
                                                                && row[i + 1].ch == ' '
                                                            {
                                                                prompt_end = i + 2;
                                                                break;
                                                            }
                                                        }
                                                    }

                                                    for (i, cell) in
                                                        row.iter().enumerate().skip(prompt_end)
                                                    {
                                                        if cell.ch != ' ' && cell.ch != '\u{0000}' {
                                                            text_end = i + 1;
                                                        }
                                                    }

                                                    // Only move right if there's text at or after the target position
                                                    let target_col = current_col + 1;
                                                    if target_col <= text_end
                                                        && target_col < state.cols
                                                    {
                                                        state.cursor_col = target_col;
                                                    }
                                                    // Don't send to PTY - handle locally
                                                }
                                            }
                                        }
                                        egui::Key::ArrowLeft => {
                                            if self.korean_state.is_composing {
                                                // 조합 중이면 조합만 완성하고 화살표는 무시
                                                self.finalize_korean_composition();
                                            } else {
                                                // DIRECT cursor movement - bypass PTY to avoid backspace issue
                                                if let Ok(mut state) = self.terminal_state.lock() {
                                                    state.set_arrow_key_protection();
                                                    let current_col = state.cursor_col;

                                                    // Find prompt end to limit leftward movement
                                                    let mut prompt_end = 0;

                                                    let row = if state.render_cursor_row
                                                        < state.render_buffer.len()
                                                    {
                                                        &state.render_buffer
                                                            [state.render_cursor_row]
                                                    } else {
                                                        return;
                                                    };

                                                    if row.len() >= 2 {
                                                        for i in 0..(row.len() - 1) {
                                                            if (row[i].ch == '~'
                                                                || row[i].ch == '✗')
                                                                && row[i + 1].ch == ' '
                                                            {
                                                                prompt_end = i + 2;
                                                                break;
                                                            }
                                                        }
                                                    }

                                                    // Only move left if we're not at prompt end
                                                    if current_col > prompt_end {
                                                        state.cursor_col = current_col - 1;
                                                    }
                                                    // Don't send to PTY - handle locally
                                                }
                                            }
                                        }
                                        _ => {
                                            // For other keys, handle normally without composition finalization
                                            if let Ok(mut writer) = self.pty_writer.lock() {
                                                match key {
                                                    egui::Key::A if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x01");
                                                        // Ctrl+A (Start of line)
                                                    }
                                                    egui::Key::B if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x02");
                                                        // Ctrl+B (Backward char)
                                                    }
                                                    egui::Key::C if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x03");
                                                        // Ctrl+C (Interrupt)
                                                    }
                                                    egui::Key::D if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x04");
                                                        // Ctrl+D (EOF)
                                                    }
                                                    egui::Key::E if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x05");
                                                        // Ctrl+E (End of line)
                                                    }
                                                    egui::Key::F if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x06");
                                                        // Ctrl+F (Forward char)
                                                    }
                                                    egui::Key::G if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x07");
                                                        // Ctrl+G (Bell)
                                                    }
                                                    egui::Key::H if modifiers.ctrl => {
                                                        // Ctrl+H is same as Backspace, but Backspace is already handled above
                                                        // Don't send duplicate
                                                        // let _ = writer.write_all(b"\x08");
                                                    }
                                                    egui::Key::I if modifiers.ctrl => {
                                                        // Ctrl+I is handled above as Tab alternative - ignore here
                                                        //println!("🔄 Ctrl+I (already handled above as Tab alternative)");
                                                    }
                                                    egui::Key::J if modifiers.ctrl => {
                                                        // Ctrl+J (Line feed) is similar to Enter
                                                        // Keep this as it's a distinct terminal control sequence
                                                        let _ = writer.write_all(b"\x0a");
                                                    }
                                                    egui::Key::K if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x0b");
                                                        // Ctrl+K (Kill line)
                                                    }
                                                    egui::Key::L if modifiers.ctrl => {
                                                        // Ctrl+L (Form Feed/Clear) - clear screen and request new prompt
                                                        if let Ok(mut state) =
                                                            self.terminal_state.lock()
                                                        {
                                                            state.clear_arrow_key_protection();
                                                            state.clear_screen();
                                                        }
                                                        // Send Ctrl+L to PTY so shell displays new prompt
                                                        let _ = writer.write_all(b"\x0c");
                                                    }
                                                    egui::Key::M if modifiers.ctrl => {
                                                        // Ctrl+M is same as Enter, but Enter is already handled above
                                                        // Don't send duplicate
                                                        // let _ = writer.write_all(b"\x0d");
                                                    }
                                                    egui::Key::N if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x0e");
                                                        // Ctrl+N (Next line)
                                                    }
                                                    egui::Key::O if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x0f");
                                                        // Ctrl+O
                                                    }
                                                    egui::Key::P if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x10");
                                                        // Ctrl+P (Previous line)
                                                    }
                                                    egui::Key::Q if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x11");
                                                        // Ctrl+Q (XON)
                                                    }
                                                    egui::Key::R if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x12");
                                                        // Ctrl+R (Reverse search)
                                                    }
                                                    egui::Key::S if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x13");
                                                        // Ctrl+S (XOFF)
                                                    }
                                                    egui::Key::T if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x14");
                                                        // Ctrl+T (Transpose)
                                                    }
                                                    egui::Key::U if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x15");
                                                        // Ctrl+U (Kill line backward)
                                                    }
                                                    egui::Key::V if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x16");
                                                        // Ctrl+V (Literal next)
                                                    }
                                                    egui::Key::W if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x17");
                                                        // Ctrl+W (Kill word backward)
                                                    }
                                                    egui::Key::X if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x18");
                                                        // Ctrl+X
                                                    }
                                                    egui::Key::Y if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x19");
                                                        // Ctrl+Y (Yank)
                                                    }
                                                    egui::Key::Z if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x1a");
                                                        // Ctrl+Z (Suspend)
                                                    }
                                                    egui::Key::Enter if modifiers.ctrl => {
                                                        let _ = writer.write_all(b"\x0d");
                                                        // Ctrl+Enter (may be useful for gemini)
                                                    }
                                                    _ => {
                                                        // For other keys, don't need special handling
                                                    }
                                                }
                                                let _ = writer.flush();
                                            }
                                        }
                                    }
                                }
                                egui::Event::Text(text) => {
                                    // Debug: Log what text events we receive (disabled for performance)
                                    // println!("🔍 Text event received: {:?} (bytes: {:?})", text, text.as_bytes());
                                    for ch in text.chars() {
                                        if ch == '\t' {
                                            // println!("⚠️ Tab character received in Text event (already handled above)");
                                            return; // Don't process as regular text - already handled above
                                        } else if ch == '\n' || ch == '\r' {
                                            // println!("⚠️ Newline/Return character received in Text event (potential duplication!): U+{:04X}", ch as u32);
                                            return; // Don't process as regular text - already handled above
                                        } else if ch == ' ' {
                                            // println!("⚠️ Space character in Text event!");
                                        } else if ch.is_ascii_graphic() {
                                            // println!("✅ Text event: '{}'", ch);
                                        } else {
                                            // println!("❓ Text event: U+{:04X} ({})", ch as u32, ch);
                                        }
                                    }
                                    // Use new IME-aware text processing
                                    self.process_text_input(text);
                                }
                                _ => {}
                            }
                        }
                    });
                }
            });
    }
}
