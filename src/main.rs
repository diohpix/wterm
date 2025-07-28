// 터미널 애플리케이션 코드 복원 - 한글 조합 지원
use anyhow::Result;
use eframe::egui;
use portable_pty::{CommandBuilder, PtySize};

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;
use vte::{Params, Parser, Perform};

// ANSI 색상 정보를 저장하는 구조체
#[derive(Clone, Debug, PartialEq)]
struct AnsiColor {
    foreground: egui::Color32,
    background: egui::Color32,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl Default for AnsiColor {
    fn default() -> Self {
        Self {
            foreground: egui::Color32::from_rgb(203, 204, 205), // Terminal white
            background: egui::Color32::TRANSPARENT,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

// 터미널 셀 정보 (문자 + 색상)
#[derive(Clone, Debug, PartialEq)]
struct TerminalCell {
    ch: char,
    color: AnsiColor,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            color: AnsiColor::default(),
        }
    }
}

// 한글 입력 관련 상수
const KOREAN_BASE: u32 = 0xAC00;

const JUNGSUNG_COUNT: u32 = 21;
const JONGSUNG_COUNT: u32 = 28;

// 초성 매핑 (자음 -> 초성 인덱스)
fn get_chosung_index(ch: char) -> Option<u32> {
    match ch {
        'ㄱ' => Some(0),
        'ㄲ' => Some(1),
        'ㄴ' => Some(2),
        'ㄷ' => Some(3),
        'ㄸ' => Some(4),
        'ㄹ' => Some(5),
        'ㅁ' => Some(6),
        'ㅂ' => Some(7),
        'ㅃ' => Some(8),
        'ㅅ' => Some(9),
        'ㅆ' => Some(10),
        'ㅇ' => Some(11),
        'ㅈ' => Some(12),
        'ㅉ' => Some(13),
        'ㅊ' => Some(14),
        'ㅋ' => Some(15),
        'ㅌ' => Some(16),
        'ㅍ' => Some(17),
        'ㅎ' => Some(18),
        _ => None,
    }
}

// 중성 매핑 (모음 -> 중성 인덱스)
fn get_jungsung_index(ch: char) -> Option<u32> {
    match ch {
        'ㅏ' => Some(0),
        'ㅐ' => Some(1),
        'ㅑ' => Some(2),
        'ㅒ' => Some(3),
        'ㅓ' => Some(4),
        'ㅔ' => Some(5),
        'ㅕ' => Some(6),
        'ㅖ' => Some(7),
        'ㅗ' => Some(8),
        'ㅘ' => Some(9),
        'ㅙ' => Some(10),
        'ㅚ' => Some(11),
        'ㅛ' => Some(12),
        'ㅜ' => Some(13),
        'ㅝ' => Some(14),
        'ㅞ' => Some(15),
        'ㅟ' => Some(16),
        'ㅠ' => Some(17),
        'ㅡ' => Some(18),
        'ㅢ' => Some(19),
        'ㅣ' => Some(20),
        _ => None,
    }
}

// 종성 매핑 (자음 -> 종성 인덱스)
fn get_jongsung_index(ch: char) -> Option<u32> {
    match ch {
        'ㄱ' => Some(1),
        'ㄲ' => Some(2),
        'ㄳ' => Some(3),
        'ㄴ' => Some(4),
        'ㄵ' => Some(5),
        'ㄶ' => Some(6),
        'ㄷ' => Some(7),
        'ㄹ' => Some(8),
        'ㄺ' => Some(9),
        'ㄻ' => Some(10),
        'ㄼ' => Some(11),
        'ㄽ' => Some(12),
        'ㄾ' => Some(13),
        'ㄿ' => Some(14),
        'ㅀ' => Some(15),
        'ㅁ' => Some(16),
        'ㅂ' => Some(17),
        'ㅄ' => Some(18),
        'ㅅ' => Some(19),
        'ㅆ' => Some(20),
        'ㅇ' => Some(21),
        'ㅈ' => Some(22),
        'ㅊ' => Some(23),
        'ㅋ' => Some(24),
        'ㅌ' => Some(25),
        'ㅍ' => Some(26),
        'ㅎ' => Some(27),
        _ => None,
    }
}

// 복합 모음 조합 (기본 모음 + 추가 모음 -> 복합 모음)
fn combine_vowels(base: char, add: char) -> Option<char> {
    match (base, add) {
        ('ㅗ', 'ㅏ') => Some('ㅘ'),
        ('ㅗ', 'ㅐ') => Some('ㅙ'),
        ('ㅗ', 'ㅣ') => Some('ㅚ'),
        ('ㅜ', 'ㅓ') => Some('ㅝ'),
        ('ㅜ', 'ㅔ') => Some('ㅞ'),
        ('ㅜ', 'ㅣ') => Some('ㅟ'),
        ('ㅡ', 'ㅣ') => Some('ㅢ'),
        _ => None,
    }
}

// 복합 자음 조합 (기본 자음 + 추가 자음 -> 복합 자음)
fn combine_consonants(base: char, add: char) -> Option<char> {
    match (base, add) {
        ('ㄱ', 'ㅅ') => Some('ㄳ'),
        ('ㄴ', 'ㅈ') => Some('ㄵ'),
        ('ㄴ', 'ㅎ') => Some('ㄶ'),
        ('ㄹ', 'ㄱ') => Some('ㄺ'),
        ('ㄹ', 'ㅁ') => Some('ㄻ'),
        ('ㄹ', 'ㅂ') => Some('ㄼ'),
        ('ㄹ', 'ㅅ') => Some('ㄽ'),
        ('ㄹ', 'ㅌ') => Some('ㄾ'),
        ('ㄹ', 'ㅍ') => Some('ㄿ'),
        ('ㄹ', 'ㅎ') => Some('ㅀ'),
        ('ㅂ', 'ㅅ') => Some('ㅄ'),
        _ => None,
    }
}

// 한글 문자 조합
fn compose_korean(chosung: u32, jungsung: u32, jongsung: u32) -> char {
    let code = KOREAN_BASE + (chosung * JUNGSUNG_COUNT + jungsung) * JONGSUNG_COUNT + jongsung;
    char::from_u32(code).unwrap_or('?')
}



// 자음 여부 확인
fn is_consonant(ch: char) -> bool {
    matches!(ch, 'ㄱ'..='ㅎ')
}

// 모음 여부 확인
fn is_vowel(ch: char) -> bool {
    matches!(ch, 'ㅏ'..='ㅣ')
}

// ANSI 256색 인덱스를 RGB로 변환 - macOS Terminal 호환
fn ansi_256_to_rgb(color_idx: u8) -> egui::Color32 {
    match color_idx {
        // Standard colors (0-15) - macOS Terminal compatible colors
        0 => egui::Color32::from_rgb(0, 0, 0),        // Black
        1 => egui::Color32::from_rgb(194, 54, 33),    // Red
        2 => egui::Color32::from_rgb(37, 188, 36),    // Green
        3 => egui::Color32::from_rgb(173, 173, 39),   // Yellow
        4 => egui::Color32::from_rgb(73, 46, 225),    // Blue
        5 => egui::Color32::from_rgb(211, 56, 211),   // Magenta
        6 => egui::Color32::from_rgb(51, 187, 200),   // Cyan
        7 => egui::Color32::from_rgb(203, 204, 205),  // White
        8 => egui::Color32::from_rgb(129, 131, 131),  // Bright Black (Gray)
        9 => egui::Color32::from_rgb(252, 57, 31),    // Bright Red
        10 => egui::Color32::from_rgb(49, 231, 34),   // Bright Green
        11 => egui::Color32::from_rgb(234, 236, 35),  // Bright Yellow
        12 => egui::Color32::from_rgb(88, 51, 255),   // Bright Blue
        13 => egui::Color32::from_rgb(249, 53, 248),  // Bright Magenta
        14 => egui::Color32::from_rgb(20, 240, 240),  // Bright Cyan
        15 => egui::Color32::from_rgb(233, 235, 235), // Bright White
        // 216 color cube (16-231)
        16..=231 => {
            let idx = color_idx - 16;
            let r = (idx / 36) % 6;
            let g = (idx / 6) % 6;
            let b = idx % 6;
            let r = if r == 0 { 0 } else { 55 + r * 40 };
            let g = if g == 0 { 0 } else { 55 + g * 40 };
            let b = if b == 0 { 0 } else { 55 + b * 40 };
            egui::Color32::from_rgb(r, g, b)
        }
        // Grayscale (232-255)
        232..=255 => {
            let gray = 8 + (color_idx - 232) * 10;
            egui::Color32::from_rgb(gray, gray, gray)
        }
    }
}

// 한글 조합 상태 관리
#[derive(Clone, Debug)]
struct KoreanInputState {
    chosung: Option<char>,  // 초성
    jungsung: Option<char>, // 중성
    jongsung: Option<char>, // 종성
    is_composing: bool,     // 조합 중인지 여부
}

impl KoreanInputState {
    fn new() -> Self {
        Self {
            chosung: None,
            jungsung: None,
            jongsung: None,
            is_composing: false,
        }
    }

    fn reset(&mut self) {
        self.chosung = None;
        self.jungsung = None;
        self.jongsung = None;
        self.is_composing = false;
    }

    // 현재 조합중인 문자 반환
    fn get_current_char(&self) -> Option<char> {
        if let (Some(cho), Some(jung)) = (self.chosung, self.jungsung) {
            let cho_idx = get_chosung_index(cho)?;
            let jung_idx = get_jungsung_index(jung)?;
            let jong_idx = self.jongsung.and_then(get_jongsung_index).unwrap_or(0);
            Some(compose_korean(cho_idx, jung_idx, jong_idx))
        } else if let Some(cho) = self.chosung {
            Some(cho)
        } else {
            None
        }
    }



    // 백스페이스 처리 - 단계별로 조합 되돌리기
    fn handle_backspace(&mut self) -> bool {
        if !self.is_composing {
            return false; // 조합 중이 아니면 처리하지 않음
        }

        // 종성이 있으면 종성부터 제거
        if self.jongsung.is_some() {
            self.jongsung = None;
            return true; // 조합 상태 유지
        }

        // 중성이 있으면 중성 제거
        if self.jungsung.is_some() {
            self.jungsung = None;
            return true; // 조합 상태 유지 (초성만 남음)
        }

        // 초성만 있으면 조합 완전 취소
        if self.chosung.is_some() {
            self.reset();
            return false; // 조합 완전 종료
        }

        false
    }
}

// 터미널 상태 구조체와 VTE 처리 코드

// Terminal state structure
#[derive(Clone)]
struct TerminalState {
    buffer: Vec<Vec<TerminalCell>>,
    cursor_row: usize,
    cursor_col: usize,
    rows: usize,
    cols: usize,
    current_color: AnsiColor,        // 현재 색상 상태

    arrow_key_pressed: bool,         // Track if arrow key was recently pressed
    arrow_key_time: Option<Instant>, // When arrow key was last pressed
    // Alternative screen buffer support
    main_buffer: Vec<Vec<TerminalCell>>, // Main screen buffer
    alt_buffer: Vec<Vec<TerminalCell>>,  // Alternative screen buffer  
    is_alt_screen: bool,                 // Currently using alternative screen
    saved_cursor_main: (usize, usize),   // Saved cursor position for main screen
    saved_cursor_alt: (usize, usize),    // Saved cursor position for alt screen
}

impl TerminalState {
    fn new(rows: usize, cols: usize) -> Self {
        let buffer = vec![vec![TerminalCell::default(); cols]; rows];
        let main_buffer = vec![vec![TerminalCell::default(); cols]; rows];
        let alt_buffer = vec![vec![TerminalCell::default(); cols]; rows];
        Self {
            buffer: buffer.clone(),
            cursor_row: 0,
            cursor_col: 0,
            rows,
            cols,
            current_color: AnsiColor::default(),
            arrow_key_pressed: false,
            arrow_key_time: None,
            main_buffer,
            alt_buffer,
            is_alt_screen: false,
            saved_cursor_main: (0, 0),
            saved_cursor_alt: (0, 0),
        }
    }

    fn clear_screen(&mut self) {
        // Clear all content and reset cursor to top-left
        for row in &mut self.buffer {
            for cell in row {
                *cell = TerminalCell::default();
            }
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == self.rows && new_cols == self.cols {
            return;
        }

        let old_rows = self.rows;
        let old_cols = self.cols;

        // Update dimensions
        self.rows = new_rows;
        self.cols = new_cols;

        // Process each buffer
        let process_buffer = |buf: &mut Vec<Vec<TerminalCell>>, saved_cursor: &mut (usize, usize)| {
            let copy_rows = new_rows.min(old_rows);
            let copy_cols = new_cols.min(old_cols);

            let row_offset = old_rows.saturating_sub(new_rows);

            let mut new_buf = vec![vec![TerminalCell::default(); new_cols]; new_rows];

            for r in 0..copy_rows {
                let old_r = row_offset + r;
                for c in 0..copy_cols {
                    new_buf[r][c] = std::mem::replace(&mut buf[old_r][c], TerminalCell::default());
                }
            }

            *buf = new_buf;

            // Adjust saved cursor
            *saved_cursor = (
                saved_cursor.0.saturating_sub(row_offset).min(new_rows.saturating_sub(1)),
                saved_cursor.1.min(new_cols.saturating_sub(1)),
            );
        };

        process_buffer(&mut self.buffer, &mut (0, 0));  // Current buffer doesn't have saved, but skip
        process_buffer(&mut self.main_buffer, &mut self.saved_cursor_main);
        process_buffer(&mut self.alt_buffer, &mut self.saved_cursor_alt);

        // Adjust current cursor
        self.cursor_row = self.cursor_row.saturating_sub(old_rows.saturating_sub(new_rows)).min(new_rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));
    }

    fn put_char(&mut self, ch: char) {
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
            self.buffer[self.cursor_row][self.cursor_col] = TerminalCell {
                ch,
                color: self.current_color.clone(),
            };

            // For wide characters (width 2), mark the second cell as a continuation
            if char_width == 2 && self.cursor_col + 1 < self.cols {
                self.buffer[self.cursor_row][self.cursor_col + 1] = TerminalCell {
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

    fn newline(&mut self) {
        // Reset arrow key state when moving to new line
        self.clear_arrow_key_protection();

        self.cursor_row += 1;
        self.cursor_col = 0;
        if self.cursor_row >= self.rows {
            // Scroll up
            self.buffer.remove(0);
            self.buffer.push(vec![TerminalCell::default(); self.cols]);
            self.cursor_row = self.rows - 1;
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_col = 0;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            // Find prompt end to prevent deleting into prompt area
            let mut prompt_end = 0;
            if self.cursor_row < self.buffer.len() {
                let row = &self.buffer[self.cursor_row];
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
                while delete_col > 0 && self.buffer[self.cursor_row][delete_col].ch == '\u{0000}' {
                    delete_col -= 1;
                }

                // Double-check we're still in user input area after finding the actual character
                if delete_col >= prompt_end {
                    // Get the character we're about to delete
                    let ch_to_delete = self.buffer[self.cursor_row][delete_col].ch;
                    let char_width = ch_to_delete.width().unwrap_or(1);

                    // Clear the character and any continuation markers
                    for i in 0..char_width {
                        if delete_col + i < self.cols {
                            self.buffer[self.cursor_row][delete_col + i] = TerminalCell::default();
                        }
                    }

                    // Move cursor to the position of the deleted character
                    self.cursor_col = delete_col;
                    println!("🔄 Backspace: cursor {} -> {} (prompt_end: {})", self.cursor_col + char_width, self.cursor_col, prompt_end);
                } else {
                    println!("🚫 Backspace blocked: would delete prompt area (delete_col: {}, prompt_end: {})", delete_col, prompt_end);
                }
            } else {
                println!("🚫 Backspace blocked: cursor at prompt area (cursor: {}, prompt_end: {})", self.cursor_col, prompt_end);
            }
        }
    }

    fn move_cursor_to(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
    }

    // Check if arrow key protection should still be active (within 300ms)
    fn should_protect_from_arrow_key(&self) -> bool {
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
    fn set_arrow_key_protection(&mut self) {
        self.arrow_key_pressed = true;
        self.arrow_key_time = Some(Instant::now());
    }

    // Clear arrow key protection
    fn clear_arrow_key_protection(&mut self) {
        self.arrow_key_pressed = false;
        self.arrow_key_time = None;
    }

    // Switch to alternative screen buffer
    fn switch_to_alt_screen(&mut self) {
        if !self.is_alt_screen {
            // Save current main screen state
            self.main_buffer = self.buffer.clone();
            self.saved_cursor_main = (self.cursor_row, self.cursor_col);
            
            // Switch to alternative screen (start with clean screen)
            // Create a completely clean alt screen buffer
            self.buffer = vec![vec![TerminalCell::default(); self.cols]; self.rows];
            self.cursor_row = 0;
            self.cursor_col = 0;
            self.is_alt_screen = true;
            
            println!("🔄 Switched to alternative screen buffer (clean screen)");
        }
    }

    // Switch back to main screen buffer
    fn switch_to_main_screen(&mut self) {
        if self.is_alt_screen {
            // Don't save alt screen state - each app gets a clean alt screen
            // Just restore main screen
            self.buffer = self.main_buffer.clone();
            self.cursor_row = self.saved_cursor_main.0;
            self.cursor_col = self.saved_cursor_main.1;
            self.is_alt_screen = false;
            
            println!("🔄 Restored main screen buffer");
        }
    }


}

// VTE Performer implementation
struct TerminalPerformer {
    state: Arc<Mutex<TerminalState>>,
}

impl TerminalPerformer {
    fn new(state: Arc<Mutex<TerminalState>>) -> Self {
        Self { state }
    }
}

impl Perform for TerminalPerformer {
    fn print(&mut self, c: char) {
        if let Ok(mut state) = self.state.lock() {
            // Log character input to help debug
            if c.is_ascii_graphic() || c == ' ' {
                print!("'{}'", c);
            } else {
                print!("U+{:04X}", c as u32);
            }
            io::stdout().flush().unwrap_or(());
            state.put_char(c);
        }
    }

    fn execute(&mut self, byte: u8) {
        if let Ok(mut state) = self.state.lock() {
            match byte {
                b'\n' => {
                    println!("📄 Newline");
                    state.newline();
                }
                b'\r' => {
                    println!("🔄 Carriage return");
                    state.carriage_return();
                }
                b'\x08' => {
                    // Backspace (Ctrl+H) - block if arrow key was recently pressed or would enter prompt
                    if state.should_protect_from_arrow_key() {
                        println!("🚫 Backspace \\x08 (blocked - arrow key protection active)");
                    } else {
                        println!("⬅️ Backspace \\x08 (processing with prompt protection)");
                        state.backspace(); // Now has prompt protection built-in
                    }
                }
                b'\x09' => {
                    // Tab character - move cursor to next tab stop (every 8 columns)
                    println!("🔄 Tab character received from PTY");
                    let next_tab_stop = ((state.cursor_col / 8) + 1) * 8;
                    if next_tab_stop < state.cols {
                        state.cursor_col = next_tab_stop;
                        println!("🔄 Tab: cursor moved to column {}", state.cursor_col);
                    } else {
                        // If tab would go beyond line, go to end of line
                        state.cursor_col = state.cols - 1;
                        println!("🔄 Tab: cursor moved to end of line ({})", state.cursor_col);
                    }
                }
                b'\x0c' => {
                    // Form Feed (Ctrl+L) - allow screen clear but clear protection first
                    state.clear_arrow_key_protection();
                    state.clear_screen();
                    println!("🧹 Form Feed - screen cleared");
                }
                b'\x7f' => {
                    // DEL character - block if arrow key was recently pressed or would enter prompt
                    if state.should_protect_from_arrow_key() {
                        println!("🚫 DEL \\x7f (blocked - arrow key protection active)");
                    } else {
                        println!("🗑️ DEL \\x7f (processing with prompt protection)");
                        state.backspace(); // Now has prompt protection built-in
                    }
                }
                _ => {
                    // Log other control characters for debugging
                    if byte < 32 {
                        println!("❓ Control char: 0x{:02x}", byte);
                    }
                }
            }
        }
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        println!(
            "🪝 HOOK: '{}' params:{:?} intermediates:{:?} ignore:{}",
            c,
            params
                .iter()
                .map(|p| p.first().copied().unwrap_or(0))
                .collect::<Vec<_>>(),
            intermediates,
            ignore
        );
    }

    fn put(&mut self, byte: u8) {
        println!(
            "📋 PUT: 0x{:02x} ('{}')",
            byte,
            if byte.is_ascii_graphic() {
                byte as char
            } else {
                '?'
            }
        );
    }

    fn unhook(&mut self) {
        println!("🔓 UNHOOK");
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        println!("🎯 OSC: params:{:?} bell:{}", params, bell_terminated);
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, c: char) {
        if let Ok(mut state) = self.state.lock() {
            // Debug: Print CSI commands to understand what's happening
            let param_values: Vec<u16> = params
                .iter()
                .map(|p| p.first().copied().unwrap_or(0))
                .collect();
            println!(
                "CSI: '{}' params:{:?} arrow_pressed:{}",
                c, param_values, state.arrow_key_pressed
            );

            // Copy values we need before the match to avoid borrowing issues
            let cols = state.cols;
            let rows = state.rows;

            match c {
                'H' | 'f' => {
                    // CUP (Cursor Position) or HVP (Horizontal and Vertical Position)
                    let row = params.iter().next().unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    let col = params.iter().nth(1).unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    state.move_cursor_to(row, col);
                }
                'J' => {
                    // ED (Erase in Display) - COMPLETELY BLOCK ALL to prevent text deletion
                    let param = params.iter().next().unwrap_or(&[0])[0];
                    match param {
                        0 => {
                            // ALWAYS BLOCK - this is the main culprit
                            println!("🚫 BLOCKED: ED 0 (clear to end of display)");
                        }
                        1 => {
                            // ALWAYS BLOCK
                            println!("🚫 BLOCKED: ED 1 (clear from start to cursor)");
                        }
                        2 => {
                            // Only allow if explicitly requested (very rare case)
                            // Block this too for now to be safe
                            println!("🚫 BLOCKED: ED 2 (clear screen - completely blocked)");
                        }
                        3 => {
                            // Block this too
                            println!(
                                "🚫 BLOCKED: ED 3 (clear screen + scrollback - completely blocked)"
                            );
                        }
                        _ => {
                            println!("🚫 BLOCKED: ED {} (unknown erase display)", param);
                        }
                    }
                }
                'K' => {
                    // EL (Erase in Line) - COMPLETELY BLOCK ALL to prevent text deletion
                    let param = params.iter().next().unwrap_or(&[0])[0];
                    match param {
                        0 => {
                            // ALWAYS BLOCK - this is the main cause of text disappearing
                            println!("🚫 BLOCKED: EL 0 (clear to end of line) - MAIN CULPRIT!");
                        }
                        1 => {
                            // ALWAYS BLOCK
                            println!("🚫 BLOCKED: EL 1 (clear from start to cursor)");
                        }
                        2 => {
                            // ALWAYS BLOCK for now - even entire line clear
                            println!("🚫 BLOCKED: EL 2 (clear entire line - completely blocked)");
                        }
                        _ => {
                            println!("🚫 BLOCKED: EL {} (unknown erase line)", param);
                        }
                    }
                }
                'A' => {
                    // CUU (Cursor Up) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_row = state.cursor_row.saturating_sub(count);
                    state.set_arrow_key_protection();
                    println!("⬆️ Cursor UP by {}", count);
                }
                'B' => {
                    // CUD (Cursor Down) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_row = (state.cursor_row + count).min(rows - 1);
                    state.set_arrow_key_protection();
                    println!("⬇️ Cursor DOWN by {}", count);
                }
                'C' => {
                    // CUF (Cursor Forward) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_col = (state.cursor_col + count).min(cols - 1);
                    state.set_arrow_key_protection();
                    println!("➡️ Cursor RIGHT by {}", count);
                }
                'D' => {
                    // CUB (Cursor Backward) - ALWAYS ALLOW cursor movement
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_col = state.cursor_col.saturating_sub(count);
                    state.set_arrow_key_protection();
                    println!("⬅️ Cursor LEFT by {}", count);
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
                                    22 => state.current_color.bold = false, // Normal intensity
                                    23 => state.current_color.italic = false, // Not italic
                                    24 => state.current_color.underline = false, // Not underlined
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
                                    // Cursor visibility mode - silently ignore
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
                    state.cursor_row = row.min(rows - 1);
                }
                'G' => {
                    // CHA (Cursor Horizontal Absolute)
                    let col = params.iter().next().unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    state.cursor_col = col.min(cols - 1);
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
                    // ECH (Erase Character) - COMPLETELY BLOCKED
                    let count = params.iter().next().unwrap_or(&[1])[0];
                    println!("🚫 BLOCKED: ECH (erase {} characters)", count);
                }
                'P' => {
                    // DCH (Delete Character) - COMPLETELY BLOCKED
                    let count = params.iter().next().unwrap_or(&[1])[0];
                    println!("🚫 BLOCKED: DCH (delete {} characters)", count);
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
                    // Save cursor position - ignore for now
                }
                'u' => {
                    // Restore cursor position - ignore for now
                }
                _ => {
                    // Silently ignore unknown CSI sequences
                    // This helps with compatibility with complex prompts
                }
            }
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

// Main terminal application
pub struct TerminalApp {
    terminal_state: Arc<Mutex<TerminalState>>,
    pty_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pty_master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    korean_state: KoreanInputState,
    last_tab_time: Option<Instant>,  // Tab key debouncing
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
        if is_consonant(ch) || is_vowel(ch) {
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
        if is_consonant(ch) {
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
                if let Some(combined) = combine_consonants(existing_jong, ch) {
                    self.korean_state.jongsung = Some(combined);
                    return None; // Still composing
                } else {
                    // Can't combine - complete current syllable and start new one
                    let completed = self.korean_state.get_current_char();
                    self.korean_state.reset();
                    self.korean_state.chosung = Some(ch);
                    self.korean_state.is_composing = true;
                    return completed; // Send completed character
                }
            } else {
                // Already have chosung but no jungsung - complete current and start new
                let completed = self.korean_state.get_current_char();
                self.korean_state.reset();
                self.korean_state.chosung = Some(ch);
                self.korean_state.is_composing = true;
                return completed; // Send completed character
            }
        } else if is_vowel(ch) {
            if self.korean_state.chosung.is_some() && self.korean_state.jungsung.is_none() {
                // We have chosung, this vowel becomes jungsung
                self.korean_state.jungsung = Some(ch);
                return None; // Still composing
            } else if let Some(existing_jung) = self.korean_state.jungsung {
                // Check if we have jongsung - if so, we need to move it to new syllable
                if let Some(jong) = self.korean_state.jongsung {
                    // Complete current syllable without the jongsung (ㄱㅏㄴ->ㄱㅏ완성, ㄴㅏ시작)
                    let cho_idx = get_chosung_index(self.korean_state.chosung.unwrap()).unwrap();
                    let jung_idx = get_jungsung_index(existing_jung).unwrap();
                    let completed = compose_korean(cho_idx, jung_idx, 0); // No jongsung

                    // Start new syllable with jongsung as chosung
                    self.korean_state.reset();
                    self.korean_state.chosung = Some(jong);
                    self.korean_state.jungsung = Some(ch);
                    self.korean_state.is_composing = true;
                    return Some(completed); // Send completed "가", keep "나" composing
                } else {
                    // Try to combine with existing jungsung
                    if let Some(combined) = combine_vowels(existing_jung, ch) {
                        self.korean_state.jungsung = Some(combined);
                        return None; // Still composing
                    } else {
                        // Can't combine - complete current syllable
                        let completed = self.korean_state.get_current_char();
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
                self.send_to_pty(&completed.to_string());
            }
            self.korean_state.reset();
        }
    }

    // Helper function to send text to PTY
    fn send_to_pty(&self, text: &str) {
        if let Ok(mut writer) = self.pty_writer.lock() {
            // Log what we're sending to PTY for debugging
            if text.starts_with('\x1b') {
                println!("📤 Sending to PTY: ESC sequence {:?}", text);
            } else {
                println!("📤 Sending to PTY: {:?}", text);
            }
            let _ = writer.write_all(text.as_bytes());
            let _ = writer.flush();
        }
    }

    fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
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

        let initial_rows = 30;
        let initial_cols = 80;
        let terminal_state = Arc::new(Mutex::new(TerminalState::new(initial_rows, initial_cols)));

        // Create PTY
        let pty_system = portable_pty::native_pty_system();
        let pty_pair = pty_system.openpty(PtySize {
            rows: initial_rows as u16,
            cols: initial_cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Spawn shell
        let mut cmd = CommandBuilder::new("zsh");
        cmd.env("TERM", "xterm-256color");
        cmd.env("LANG", "ko_KR.UTF-8");
        cmd.env("LC_ALL", "ko_KR.UTF-8");
        cmd.env("LC_CTYPE", "UTF-8");
        cmd.env("SHELL", "/bin/zsh");
        cmd.env("COLORTERM", "truecolor");
        // Fix arrow key mapping issues
        cmd.env("INPUTRC", "/dev/null"); // Ignore custom readline config
        cmd.env("ZSH_NO_EXEC", "0"); // Ensure zsh processes commands normally
        let _child = pty_pair.slave.spawn_command(cmd)?;

        let mut pty_reader = pty_pair.master.try_clone_reader()?;
        let pty_writer = Arc::new(Mutex::new(pty_pair.master.take_writer()?));
        let pty_master = Arc::new(Mutex::new(pty_pair.master));

        // Spawn background thread to read from PTY
        let state_clone = terminal_state.clone();
        thread::spawn(move || {
            let mut parser = Parser::new();
            let mut performer = TerminalPerformer::new(state_clone);

            let mut buffer = [0u8; 1024];
            loop {
                match pty_reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        // Log raw PTY data for debugging
                        print!("📡 PTY Raw: ");
                        for &byte in &buffer[..n] {
                            if byte.is_ascii_graphic() || byte == b' ' {
                                print!("'{}'", byte as char);
                            } else {
                                print!("0x{:02x} ", byte);
                            }
                        }
                        println!();
                        io::stdout().flush().unwrap_or(());

                        // Process all bytes at once using VTE 0.15 API
                        parser.advance(&mut performer, &buffer[..n]);
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            terminal_state,
            pty_writer,
            pty_master,
            korean_state: KoreanInputState::new(),
            last_tab_time: None,
        })
    }

    fn calculate_terminal_size(&self, available_rect: egui::Rect, ui: &egui::Ui) -> (usize, usize) {
        let font_id = egui::FontId::new(11.0, egui::FontFamily::Monospace);
        let line_height = ui.fonts(|f| f.row_height(&font_id));
        let char_width = ui.fonts(|f| f.glyph_width(&font_id, ' '));

        // Use most of the available space, leaving small margin for scrollbar
        let usable_height = available_rect.height() - 20.0; // Small margin for scrollbar
        let usable_width = available_rect.width() - 20.0; // Small margin for scrollbar

        let rows = (usable_height / line_height).floor() as usize;
        let cols = (usable_width / char_width).floor() as usize;

        // Minimum size constraints
        let rows = rows.max(10);
        let cols = cols.max(40);

        (rows, cols)
    }

    fn resize_terminal(&mut self, new_rows: usize, new_cols: usize) -> Result<()> {
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
            let mut state = self.terminal_state.lock().unwrap();
            state.resize(new_rows, new_cols);
        }

        // Resize the PTY
        {
            let pty_master = self.pty_master.lock().unwrap();
            let new_size = PtySize {
                rows: new_rows as u16,
                cols: new_cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            };

            pty_master.resize(new_size).map_err(|e| {
                eprintln!("Failed to resize PTY: {}", e);
                anyhow::anyhow!("PTY resize failed: {}", e)
            })?;
        }

        Ok(())
    }
}

impl eframe::App for TerminalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // No need to check IME timeout with rustkorean

        egui::CentralPanel::default().show(ctx, |ui| {
            // Show terminal info
            ui.horizontal(|ui| {
                ui.label("🖥️ WTerm:");
                ui.label("macOS 스타일 터미널");
            });

            ui.separator();

            // Calculate available space for terminal after header and info
            let remaining_rect = ui.available_rect_before_wrap();

            // Calculate terminal size based on the remaining space
            let (terminal_rows, terminal_cols) = self.calculate_terminal_size(remaining_rect, ui);

            // Resize terminal if needed
            self.resize_terminal(terminal_rows, terminal_cols).unwrap();

            // Terminal display with focus handling and proper scrolling
            let terminal_response = egui::ScrollArea::vertical()
                .id_salt("terminal_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // Calculate exact font metrics
                    let font_id = egui::FontId::new(11.0, egui::FontFamily::Monospace);
                    let line_height = ui.fonts(|f| f.row_height(&font_id));
                    // Use a consistent character for width calculation (use 'M' for monospace)
                    let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'M'));

                    // Calculate terminal content size
                    if let Ok(state) = self.terminal_state.lock() {
                        let content_height = state.rows as f32 * line_height;
                        let content_width = state.cols as f32 * char_width;

                        // Allocate exact space needed for terminal content with keyboard focus
                        let (response, painter) = ui.allocate_painter(
                            egui::Vec2::new(content_width, content_height),
                            egui::Sense::click_and_drag().union(egui::Sense::focusable_noninteractive()),
                        );

                        // Draw terminal background (macOS Terminal style black background)
                        painter.rect_filled(
                            response.rect,
                            egui::CornerRadius::ZERO,
                            egui::Color32::BLACK,
                        );

                        // Request focus when clicked and claim keyboard input
                        if response.clicked() {
                            println!("🔍 DEBUG: Terminal clicked - requesting focus (ID: {:?})", response.id);
                            ui.memory_mut(|mem| mem.request_focus(response.id));
                        }
                        
                        // Always try to maintain focus on the terminal
                        if response.hovered() || response.has_focus() {
                            ui.memory_mut(|mem| mem.request_focus(response.id));
                        }
                        
                        // Force focus when any interaction happens
                        let interaction = response.interact(egui::Sense::click_and_drag());
                        if interaction.clicked() || interaction.dragged() || interaction.hovered() {
                            ui.memory_mut(|mem| mem.request_focus(response.id));
                        }
                        
                        // If focused, we will handle keyboard input in the event loop

                        // Terminal rendering without debug output

                        // First, draw all terminal content (characters and backgrounds)
                        for (row_idx, row) in state.buffer.iter().enumerate() {
                            let y = response.rect.top() + row_idx as f32 * line_height;
                            let mut col_offset = 0.0;

                            for (_col_idx, cell) in row.iter().enumerate() {
                                // Skip continuation markers for wide characters
                                if cell.ch == '\u{0000}' {
                                    continue;
                                }

                                // For monospace font, all characters should have same width except for wide chars
                                let char_display_width = if cell.ch.width().unwrap_or(1) == 2 {
                                    2 // Keep wide characters (like Korean) as 2 units
                                } else {
                                    1 // All other characters (including space) are 1 unit
                                };
                                let display_width = char_display_width as f32 * char_width;

                                let x = response.rect.left() + col_offset;
                                let pos = egui::Pos2::new(x, y);
                                let cell_rect = egui::Rect::from_min_size(
                                    pos,
                                    egui::Vec2::new(display_width, line_height),
                                );

                                // Draw background color if not transparent and not the default black
                                if cell.color.background != egui::Color32::TRANSPARENT
                                    && cell.color.background != egui::Color32::BLACK
                                {
                                    painter.rect_filled(
                                        cell_rect,
                                        egui::CornerRadius::ZERO,
                                        cell.color.background,
                                    );
                                }

                                // Normal character rendering (don't draw cursor here)
                                if cell.ch != ' '
                                    || (cell.color.background != egui::Color32::TRANSPARENT
                                        && cell.color.background != egui::Color32::BLACK)
                                {
                                    let mut text_color = cell.color.foreground;

                                    // Apply bold effect by making color brighter
                                    if cell.color.bold {
                                        let [r, g, b, a] = text_color.to_array();
                                        text_color = egui::Color32::from_rgba_unmultiplied(
                                            (r as f32 * 1.3).min(255.0) as u8,
                                            (g as f32 * 1.3).min(255.0) as u8,
                                            (b as f32 * 1.3).min(255.0) as u8,
                                            a,
                                        );
                                    }

                                    if cell.ch != ' ' {
                                        painter.text(
                                            pos,
                                            egui::Align2::LEFT_TOP,
                                            cell.ch,
                                            font_id.clone(),
                                            text_color,
                                        );
                                    }

                                    // Draw underline if enabled
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

                        // Now draw cursor separately at correct position
                        let cursor_y = response.rect.top() + state.cursor_row as f32 * line_height;

                        // Calculate precise cursor X position by walking through the row
                        let mut cursor_x = response.rect.left();
                        if state.cursor_row < state.buffer.len() {
                            for (col_idx, cell) in state.buffer[state.cursor_row].iter().enumerate()
                            {
                                if col_idx >= state.cursor_col {
                                    break;
                                }

                                // Skip continuation markers for wide characters
                                if cell.ch == '\u{0000}' {
                                    continue;
                                }

                                // For monospace font, all characters should have same width except for wide chars
                                let char_display_width = if cell.ch.width().unwrap_or(1) == 2 {
                                    2 // Keep wide characters (like Korean) as 2 units
                                } else {
                                    1 // All other characters (including space) are 1 unit
                                };
                                cursor_x += char_display_width as f32 * char_width;
                            }
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
                                // Draw composing character with a different color (gray/dimmed) to show it's temporary
                                let preview_color = egui::Color32::from_rgb(150, 150, 150); // Gray preview color
                                
                                painter.text(
                                    egui::Pos2::new(cursor_x, cursor_y),
                                    egui::Align2::LEFT_TOP,
                                    composing_char,
                                    font_id.clone(),
                                    preview_color,
                                );
                                
                                // Draw a subtle background to make the preview more visible
                                let preview_bg = egui::Color32::from_rgba_unmultiplied(100, 100, 100, 50);
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::Pos2::new(cursor_x, cursor_y),
                                        egui::Vec2::new(cursor_width, line_height),
                                    ),
                                    egui::CornerRadius::ZERO,
                                    preview_bg,
                                );
                                
                                // Redraw the composing character on top of background
                                painter.text(
                                    egui::Pos2::new(cursor_x, cursor_y),
                                    egui::Align2::LEFT_TOP,
                                    composing_char,
                                    font_id.clone(),
                                    preview_color,
                                );
                            }
                        }

                        // Underscore cursor style - doesn't cover text
                        let cursor_color = egui::Color32::WHITE;

                        // Only draw cursor if we're actually at a valid position
                        if state.cursor_row < state.buffer.len() && state.cursor_col < state.cols {
                            // Draw underscore cursor at the bottom of the character cell
                            let cursor_line_y = cursor_y + line_height - 2.0; // 2 pixels from bottom
                            let cursor_line_thickness = 2.0;

                            painter.rect_filled(
                                egui::Rect::from_min_size(
                                    egui::Pos2::new(cursor_x, cursor_line_y),
                                    egui::Vec2::new(cursor_width, cursor_line_thickness),
                                ),
                                egui::CornerRadius::ZERO,
                                cursor_color,
                            );
                        }

                        // Auto-scroll to cursor position
                        let cursor_y = state.cursor_row as f32 * line_height;
                        let cursor_rect = egui::Rect::from_min_size(
                            egui::Pos2::new(0.0, cursor_y),
                            egui::Vec2::new(char_width, line_height),
                        );
                        ui.scroll_to_rect(cursor_rect, Some(egui::Align::Center));

                        response
                    } else {
                        ui.allocate_response(egui::Vec2::new(800.0, 600.0), egui::Sense::click())
                    }
                });

            // Handle keyboard input when terminal has focus
            let has_focus = ui.memory(|mem| mem.has_focus(terminal_response.inner.id));
            


            // Handle Tab key with raw event processing and debouncing
            let tab_handled = ctx.input_mut(|i| {
                let mut tab_press_found = false;
                let mut tab_release_found = false;
                
                // Debug: Count total events and Tab events
                let total_events = i.events.len();
                let mut tab_events = 0;
                
                // Process all events and consume Tab events to prevent UI focus changes
                i.events.retain(|event| {
                    match event {
                        egui::Event::Key { key: egui::Key::Tab, pressed: true, .. } => {
                            tab_events += 1;
                            tab_press_found = true;
                            println!("🔍 DEBUG: Tab key PRESS detected! (focus: {}, event #{}/{})", has_focus, tab_events, total_events);
                            false // Always consume Tab events to prevent focus changes
                        }
                        egui::Event::Key { key: egui::Key::Tab, pressed: false, .. } => {
                            tab_events += 1;
                            tab_release_found = true;
                            println!("🔍 DEBUG: Tab key RELEASE detected! (event #{}/{}) - NOT SENDING", tab_events, total_events);
                            false // Also consume Tab release events
                        }
                        egui::Event::Key { key, pressed, .. } => {
                            if *key == egui::Key::I {
                                println!("🔍 DEBUG: I key event - key:{:?} pressed:{}", key, pressed);
                            }
                            true
                        }
                        _ => true
                    }
                });
                
                // Only handle Tab PRESS, ignore RELEASE to prevent duplicate sending
                if tab_press_found {
                    println!("✅ Tab PRESS detected - will send to PTY");
                    true
                } else if tab_release_found {
                    println!("🚫 Tab RELEASE detected - IGNORING to prevent duplicate");
                    false  // Don't handle release to prevent duplicate
                } else {
                    // Only log when no Tab events in busy periods
                    if total_events > 0 && tab_events == 0 && total_events < 3 {
                        println!("🔍 DEBUG: {} events processed, but no Tab events found", total_events);
                    }
                    false
                }
            });
            
            // Send Tab to PTY with debouncing (only if enough time has passed since last Tab)
            if tab_handled {
                let now = Instant::now();
                let should_send = if let Some(last_time) = self.last_tab_time {
                    let elapsed = now.duration_since(last_time).as_millis();
                    println!("🔍 DEBUG: Tab debounce check - elapsed: {}ms", elapsed);
                    elapsed > 100 // 100ms debounce (reduced from 200ms)
                } else {
                    println!("🔍 DEBUG: First Tab key press");
                    true // First Tab key
                };
                
                if should_send {
                    println!("📤 Sending Tab for auto-completion (focus was: {})", has_focus);
                    // Ensure terminal has focus before and after sending Tab
                    ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                    self.finalize_korean_composition();
                    self.send_to_pty("\t");
                    self.last_tab_time = Some(now);
                    // Force focus again after sending Tab to prevent losing focus
                    ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                    println!("✅ Tab sent successfully, focus maintained");
                } else {
                    println!("🚫 Tab debounced (too frequent - try again in {}ms)", 100 - now.duration_since(self.last_tab_time.unwrap()).as_millis());
                }
            }

            // Handle ESC key specially using direct input check
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                println!("🔍 DEBUG: ESC key pressed (focus was: {}, composing: {})", has_focus, self.korean_state.is_composing);
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
                    println!("🔍 DEBUG: Ctrl+I debounce check - elapsed: {}ms", elapsed);
                    elapsed > 100 // 100ms debounce (reduced from 200ms)
                } else {
                    println!("🔍 DEBUG: First Ctrl+I key press");
                    true // First Ctrl+I
                };
                
                if should_send {
                    println!("📤 Ctrl+I detected - sending as Tab for auto-completion (focus was: {})", has_focus);
                    // Ensure terminal has focus before and after sending Tab
                    ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                    self.finalize_korean_composition();
                    self.send_to_pty("\t");
                    self.last_tab_time = Some(now);
                    // Force focus again after sending Tab to prevent losing focus
                    ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
                    println!("✅ Ctrl+I sent successfully, focus maintained");
                } else {
                    println!("🚫 Ctrl+I debounced (too frequent - try again in {}ms)", 100 - now.duration_since(self.last_tab_time.unwrap()).as_millis());
                }
            }

            if has_focus {
                ctx.input(|i| {
                    // Debug: Log events only when relevant
                    let total_events = i.events.len();
                    if total_events > 0 && total_events < 3 {
                        println!("🔍 DEBUG: Processing {} input events in key handler", total_events);
                    }
                    
                    for event in &i.events {
                        match event {
                            egui::Event::Key { key, pressed, modifiers, .. } => {
                                if *key == egui::Key::Tab {
                                    println!("🔍 DEBUG: Tab key in key handler - key:{:?} pressed:{} modifiers:{:?}", key, pressed, modifiers);
                                }
                                if *key == egui::Key::I && modifiers.ctrl {
                                    println!("🔍 DEBUG: Ctrl+I key event detected - key:{:?} pressed:{} modifiers:{:?}", key, pressed, modifiers);
                                }
                            }
                            egui::Event::Text(text) => {
                                if text.contains('\t') {
                                    println!("🔍 DEBUG: Tab character in text event: {:?}", text);
                                }
                            }
                            _ => {}
                        }
                    }
                    
                    for event in &i.events {
                        match event {
                            egui::Event::Key {
                                key,
                                pressed: true,
                                modifiers,
                                ..
                            } => {
                                // Skip Tab keys completely - they're handled above
                                if *key == egui::Key::Tab {
                                    println!("🔍 DEBUG: Tab key in key handler (SKIPPED - handled above)");
                                    continue;
                                }
                                
                                // Debug: Log all other key events
                                println!("🔑 Key event: {:?} (modifiers: {:?})", key, modifiers);
                                // Handle keys that should finalize Korean composition
                                match key {
                                    egui::Key::Enter => {
                                        self.finalize_korean_composition();
                                        // Reset arrow key state when user presses Enter
                                        if let Ok(mut state) = self.terminal_state.lock() {
                                            state.clear_arrow_key_protection();
                                        }
                                        self.send_to_pty("\n");
                                    }
                                    egui::Key::Space => {
                                        // Space is handled by Text event, don't handle it here
                                        // Just finalize Korean composition if any
                                        self.finalize_korean_composition();
                                    }
                                    // Tab is handled above - no case needed here

                                    egui::Key::Backspace => {
                                        // Handle backspace for Korean composition
                                        if self.korean_state.is_composing {
                                            // Step-by-step Korean composition backspace
                                            let still_composing =
                                                self.korean_state.handle_backspace();
                                            if !still_composing {
                                                // Composition ended, handle backspace directly with prompt protection
                                                if let Ok(mut state) = self.terminal_state.lock() {
                                                    state.clear_arrow_key_protection();
                                                    state.backspace();
                                                }
                                            }
                                            // If still_composing is true, just update visual without any terminal operation
                                        } else {
                                            // Handle backspace directly with prompt protection (no PTY round-trip)
                                            if let Ok(mut state) = self.terminal_state.lock() {
                                                state.clear_arrow_key_protection();
                                                state.backspace();
                                            }
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
                                                if state.cursor_row < state.buffer.len() {
                                                    let row = &state.buffer[state.cursor_row];
                                                    // Find prompt end: "~ " or "✗ " pattern
                                                    for i in 0..row.len().saturating_sub(1) {
                                                        if (row[i].ch == '~' || row[i].ch == '✗') && row[i + 1].ch == ' ' {
                                                            prompt_end = i + 2; // Position after "~ " or "✗ "
                                                            break;
                                                        }
                                                    }
                                                    
                                                    // Find text end in user input area only
                                                    for (i, cell) in row.iter().enumerate().skip(prompt_end) {
                                                        if cell.ch != ' ' && cell.ch != '\u{0000}' {
                                                            text_end = i + 1; // Position after last non-space character
                                                        }
                                                    }
                                                }

                                                // Only move right if there's text at or after the target position
                                                let target_col = current_col + 1;
                                                if target_col <= text_end && target_col < state.cols {
                                                    state.cursor_col = target_col;
                                                    println!("🔄 Direct cursor RIGHT: {} -> {} (user text_end: {})", current_col, state.cursor_col, text_end);
                                                } else {
                                                    println!("🚫 RIGHT blocked: {} (user text_end: {}, would go beyond user text)", current_col, text_end);
                                                }
                                            }
                                            // Don't send to PTY - handle locally
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
                                                if state.cursor_row < state.buffer.len() {
                                                    let row = &state.buffer[state.cursor_row];
                                                    // Find prompt end: "~ " or "✗ " pattern
                                                    for i in 0..row.len().saturating_sub(1) {
                                                        if (row[i].ch == '~' || row[i].ch == '✗') && row[i + 1].ch == ' ' {
                                                            prompt_end = i + 2; // Position after "~ " or "✗ "
                                                            break;
                                                        }
                                                    }
                                                }

                                                // Only move left if we're not at prompt end
                                                if current_col > prompt_end {
                                                    state.cursor_col = current_col - 1;
                                                    println!("🔄 Direct cursor LEFT: {} -> {} (prompt_end: {})", current_col, state.cursor_col, prompt_end);
                                                } else {
                                                    println!("🚫 LEFT blocked: {} (prompt_end: {}, would enter prompt area)", current_col, prompt_end);
                                                }
                                            }
                                            // Don't send to PTY - handle locally
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
                                                    println!("🔄 Ctrl+I (already handled above as Tab alternative)");
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
                                                    println!("🧹 Ctrl+L: Screen cleared, requesting new prompt");
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
                                // Debug: Log what text events we receive
                                for ch in text.chars() {
                                    if ch == '\t' {
                                        println!("⚠️ Tab character received in Text event (already handled above)");
                                        return; // Don't process as regular text - already handled above
                                    } else if ch == '\n' {
                                        println!("⚠️ Newline character received in Text event (potential duplication!)");
                                    } else if ch == ' ' {
                                        println!("✅ Space character in Text event (expected)");
                                    } else if ch.is_ascii_graphic() {
                                        println!("✅ Text event: '{}'", ch);
                                    } else {
                                        println!("❓ Text event: U+{:04X}", ch as u32);
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

            // Show focus hint
            if !ui.memory(|mem| mem.has_focus(terminal_response.inner.id)) {
                ui.label("💡 터미널 영역을 클릭해서 포커스를 주세요 (Ctrl+L: 화면 클리어)");
            } else {
                ui.label("✅ 터미널 활성화됨");
            }
        });

        // Request repaint to keep updating
        ctx.request_repaint();
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_resizable(true) // Make window resizable
            .with_title("WTerm - 터미널"), // Window title
        ..Default::default()
    };

    let _result = eframe::run_native(
        "WTerm",
        options,
        Box::new(|cc| {
            Ok(Box::new(
                TerminalApp::new(cc).expect("Failed to create terminal app"),
            ))
        }),
    );
}
