// 터미널 애플리케이션 코드 복원 - create_double_consonant + compose_korean 사용
use anyhow::Result;
use eframe::egui;
use portable_pty::{CommandBuilder, PtySize};

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
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
            foreground: egui::Color32::WHITE,
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
const CHOSUNG_COUNT: u32 = 19;
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

// 한글 문자 분해
fn decompose_korean(ch: char) -> Option<(u32, u32, u32)> {
    let code = ch as u32;
    if code >= KOREAN_BASE && code < KOREAN_BASE + CHOSUNG_COUNT * JUNGSUNG_COUNT * JONGSUNG_COUNT {
        let base = code - KOREAN_BASE;
        let chosung = base / (JUNGSUNG_COUNT * JONGSUNG_COUNT);
        let jungsung = (base % (JUNGSUNG_COUNT * JONGSUNG_COUNT)) / JONGSUNG_COUNT;
        let jongsung = base % JONGSUNG_COUNT;
        Some((chosung, jungsung, jongsung))
    } else {
        None
    }
}

// 자음 여부 확인
fn is_consonant(ch: char) -> bool {
    matches!(ch, 'ㄱ'..='ㅎ')
}

// 모음 여부 확인
fn is_vowel(ch: char) -> bool {
    matches!(ch, 'ㅏ'..='ㅣ')
}

// ANSI 256색 인덱스를 RGB로 변환
fn ansi_256_to_rgb(color_idx: u8) -> egui::Color32 {
    match color_idx {
        // Standard colors (0-15)
        0 => egui::Color32::BLACK,
        1 => egui::Color32::from_rgb(128, 0, 0),
        2 => egui::Color32::from_rgb(0, 128, 0),
        3 => egui::Color32::from_rgb(128, 128, 0),
        4 => egui::Color32::from_rgb(0, 0, 128),
        5 => egui::Color32::from_rgb(128, 0, 128),
        6 => egui::Color32::from_rgb(0, 128, 128),
        7 => egui::Color32::from_rgb(192, 192, 192),
        8 => egui::Color32::from_rgb(128, 128, 128),
        9 => egui::Color32::from_rgb(255, 0, 0),
        10 => egui::Color32::from_rgb(0, 255, 0),
        11 => egui::Color32::from_rgb(255, 255, 0),
        12 => egui::Color32::from_rgb(0, 0, 255),
        13 => egui::Color32::from_rgb(255, 0, 255),
        14 => egui::Color32::from_rgb(0, 255, 255),
        15 => egui::Color32::WHITE,
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

    // 조합 완료 여부 확인
    fn is_complete(&self) -> bool {
        self.chosung.is_some() && self.jungsung.is_some()
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
    current_color: AnsiColor, // 현재 색상 상태
}

impl TerminalState {
    fn new(rows: usize, cols: usize) -> Self {
        let buffer = vec![vec![TerminalCell::default(); cols]; rows];
        Self {
            buffer,
            cursor_row: 0,
            cursor_col: 0,
            rows,
            cols,
            current_color: AnsiColor::default(),
        }
    }

    fn clear_screen(&mut self) {
        println!("DEBUG: Clearing screen");
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

        // Resize the buffer
        self.buffer
            .resize(new_rows, vec![TerminalCell::default(); new_cols]);
        for row in &mut self.buffer {
            row.resize(new_cols, TerminalCell::default());
        }

        // Update dimensions
        self.rows = new_rows;
        self.cols = new_cols;

        // Adjust cursor position if necessary
        self.cursor_row = self.cursor_row.min(new_rows - 1);
        self.cursor_col = self.cursor_col.min(new_cols - 1);
    }

    fn put_char(&mut self, ch: char) {
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
            // Move cursor back to find the character to delete
            let mut delete_col = self.cursor_col - 1;

            // If we're on a continuation marker (\u{0000}), move back to the actual character
            while delete_col > 0 && self.buffer[self.cursor_row][delete_col].ch == '\u{0000}' {
                delete_col -= 1;
            }

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
        }
    }

    fn move_cursor_to(&mut self, row: usize, col: usize) {
        println!("DEBUG: Moving cursor to ({}, {})", row, col);
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
    }

    fn clear_from_cursor_to_end(&mut self) {
        println!(
            "DEBUG: Clear from cursor to end of screen at ({}, {})",
            self.cursor_row, self.cursor_col
        );
        // Only clear from cursor position to end of current line
        // Don't clear the lines below if there's already content there
        if self.cursor_row < self.buffer.len() {
            for col in self.cursor_col..self.cols {
                if col < self.buffer[self.cursor_row].len() {
                    self.buffer[self.cursor_row][col] = TerminalCell::default();
                }
            }
        }
        // Only clear empty lines below, not lines with content
        // This prevents clearing ls output when zsh redraws its prompt
    }

    fn clear_from_start_to_cursor(&mut self) {
        println!("DEBUG: Clear from start to cursor");
        // Clear all lines above current line
        for row in 0..=self.cursor_row {
            let end_col = if row == self.cursor_row {
                self.cursor_col
            } else {
                self.cols - 1
            };
            for col in 0..=end_col {
                if row < self.buffer.len() && col < self.buffer[row].len() {
                    self.buffer[row][col] = TerminalCell::default();
                }
            }
        }
        // Clear from start of current line to cursor
        for col in 0..=self.cursor_col.min(self.cols.saturating_sub(1)) {
            if self.cursor_row < self.buffer.len() {
                self.buffer[self.cursor_row][col] = TerminalCell::default();
            }
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
            state.put_char(c);
        }
    }

    fn execute(&mut self, byte: u8) {
        if let Ok(mut state) = self.state.lock() {
            match byte {
                b'\n' => state.newline(),
                b'\r' => state.carriage_return(),
                b'\x08' => state.backspace(), // Backspace
                b'\x0c' => {
                    println!("DEBUG: Form Feed (Ctrl+L) received");
                    state.clear_screen();
                } // Form Feed (Ctrl+L)
                _ => {
                    if byte < 32 {
                        println!("DEBUG: Unknown control character: 0x{:02x}", byte);
                    }
                }
            }
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _c: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, c: char) {
        if let Ok(mut state) = self.state.lock() {
            // Copy values we need before the match to avoid borrowing issues
            let cursor_row = state.cursor_row;
            let cursor_col = state.cursor_col;
            let cols = state.cols;
            let rows = state.rows;

            match c {
                'H' | 'f' => {
                    // CUP (Cursor Position) or HVP (Horizontal and Vertical Position)
                    let row = params.iter().next().unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    let col = params.iter().nth(1).unwrap_or(&[1])[0].saturating_sub(1) as usize;
                    state.move_cursor_to(row, col);
                    println!("DEBUG: Moving cursor to ({}, {})", row, col);
                }
                'J' => {
                    // ED (Erase in Display)
                    let param = params.iter().next().unwrap_or(&[0])[0];
                    println!("DEBUG: ED (Erase in Display) with param: {}", param);
                    match param {
                        0 => {
                            println!(
                                "DEBUG: Clear from cursor to end of screen at ({}, {})",
                                cursor_row, cursor_col
                            );
                            state.clear_from_cursor_to_end();
                        }
                        1 => {
                            println!("DEBUG: Clear from start to cursor");
                            state.clear_from_start_to_cursor();
                        }
                        2 => {
                            println!("DEBUG: ED 2 - Clear entire screen");
                            state.clear_screen();
                        }
                        3 => {
                            println!("DEBUG: ED 3 - Clear entire screen and scrollback");
                            state.clear_screen();
                        }
                        _ => {}
                    }
                }
                'K' => {
                    // EL (Erase in Line)
                    let param = params.iter().next().unwrap_or(&[0])[0];
                    println!("DEBUG: EL (Erase in Line) with param: {}", param);
                    match param {
                        0 => {
                            // Clear from cursor to end of line
                            for col in cursor_col..cols {
                                if cursor_row < state.buffer.len()
                                    && col < state.buffer[cursor_row].len()
                                {
                                    state.buffer[cursor_row][col] = TerminalCell::default();
                                }
                            }
                        }
                        1 => {
                            // Clear from start of line to cursor
                            for col in 0..=cursor_col {
                                if cursor_row < state.buffer.len()
                                    && col < state.buffer[cursor_row].len()
                                {
                                    state.buffer[cursor_row][col] = TerminalCell::default();
                                }
                            }
                        }
                        2 => {
                            // Clear entire line
                            if cursor_row < state.buffer.len() {
                                for col in 0..cols {
                                    if col < state.buffer[cursor_row].len() {
                                        state.buffer[cursor_row][col] = TerminalCell::default();
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                'A' => {
                    // CUU (Cursor Up)
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_row = state.cursor_row.saturating_sub(count);
                    println!("DEBUG: Cursor Up by {}", count);
                }
                'B' => {
                    // CUD (Cursor Down)
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_row = (state.cursor_row + count).min(rows - 1);
                    println!("DEBUG: Cursor Down by {}", count);
                }
                'C' => {
                    // CUF (Cursor Forward)
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_col = (state.cursor_col + count).min(cols - 1);
                    println!("DEBUG: Cursor Forward by {}", count);
                }
                'D' => {
                    // CUB (Cursor Backward)
                    let count = params.iter().next().unwrap_or(&[1])[0] as usize;
                    state.cursor_col = state.cursor_col.saturating_sub(count);
                    println!("DEBUG: Cursor Backward by {}", count);
                }
                'm' => {
                    // SGR (Select Graphic Rendition) - colors and text attributes
                    if params.is_empty() {
                        // Reset to defaults
                        state.current_color = AnsiColor::default();
                    } else {
                        for param in params.iter() {
                            if let Some(&code) = param.first() {
                                match code {
                                    0 => state.current_color = AnsiColor::default(), // Reset
                                    1 => state.current_color.bold = true,            // Bold
                                    3 => state.current_color.italic = true,          // Italic
                                    4 => state.current_color.underline = true,       // Underline
                                    22 => state.current_color.bold = false, // Normal intensity
                                    23 => state.current_color.italic = false, // Not italic
                                    24 => state.current_color.underline = false, // Not underlined
                                    // Foreground colors (8-color)
                                    30 => state.current_color.foreground = egui::Color32::BLACK,
                                    31 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(170, 0, 0)
                                    }
                                    32 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(0, 170, 0)
                                    }
                                    33 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(170, 85, 0)
                                    }
                                    34 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(0, 0, 170)
                                    }
                                    35 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(170, 0, 170)
                                    }
                                    36 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(0, 170, 170)
                                    }
                                    37 => {
                                        state.current_color.foreground = egui::Color32::LIGHT_GRAY
                                    }
                                    // Bright foreground colors
                                    90 => state.current_color.foreground = egui::Color32::GRAY,
                                    91 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(255, 85, 85)
                                    }
                                    92 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(85, 255, 85)
                                    }
                                    93 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(255, 255, 85)
                                    }
                                    94 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(85, 85, 255)
                                    }
                                    95 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(255, 85, 255)
                                    }
                                    96 => {
                                        state.current_color.foreground =
                                            egui::Color32::from_rgb(85, 255, 255)
                                    }
                                    97 => state.current_color.foreground = egui::Color32::WHITE,
                                    // Default foreground
                                    39 => state.current_color.foreground = egui::Color32::WHITE,
                                    // Background colors (40-47, 100-107) can be added similarly
                                    // 256-color and RGB support can be added for codes 38 and 48
                                    _ => {
                                        // Handle 256-color and RGB sequences
                                        if code == 38 && param.len() >= 3 {
                                            if param[1] == 5 && param.len() >= 3 {
                                                // 256-color foreground: ESC[38;5;nm
                                                let color_idx = param[2] as u8;
                                                state.current_color.foreground =
                                                    ansi_256_to_rgb(color_idx);
                                            } else if param[1] == 2 && param.len() >= 5 {
                                                // RGB foreground: ESC[38;2;r;g;bm
                                                let r = param[2] as u8;
                                                let g = param[3] as u8;
                                                let b = param[4] as u8;
                                                state.current_color.foreground =
                                                    egui::Color32::from_rgb(r, g, b);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                'h' | 'l' => {
                    // Set Mode (h) / Reset Mode (l) - often used for terminal features
                    if let Some(first_param) = params.iter().next() {
                        let mode = first_param[0];
                        match mode {
                            1 => {
                                // Application cursor keys mode - silently ignore
                            }
                            2004 => {
                                // Bracketed paste mode - silently ignore
                            }
                            _ => {
                                // Only log unknown modes to reduce noise
                                if mode != 1 && mode != 2004 {
                                    println!(
                                        "DEBUG: Unknown mode sequence: '{}' with mode {}",
                                        c, mode
                                    );
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
                    // Window manipulation sequences - ignore for now
                }
                _ => {
                    println!(
                        "DEBUG: Unknown CSI sequence: '{}' with params {:?}",
                        c, params
                    );
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
}

impl TerminalApp {
    // Process text input with Korean composition support
    fn process_text_input(&mut self, text: &str) {
        println!("DEBUG: Processing text: '{}'", text);

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
        println!(
            "DEBUG: Processing Korean char: '{}', current state: {:?}",
            ch, self.korean_state
        );
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
        println!(
            "DEBUG: finalize_korean_composition called, is_composing: {}",
            self.korean_state.is_composing
        );
        if self.korean_state.is_composing {
            if let Some(completed) = self.korean_state.get_current_char() {
                println!("DEBUG: Completing Korean character: '{}'", completed);
                self.send_to_pty(&completed.to_string());
            }
            println!("DEBUG: Resetting Korean state");
            self.korean_state.reset();
        }
    }

    // Helper function to send text to PTY
    fn send_to_pty(&self, text: &str) {
        if let Ok(mut writer) = self.pty_writer.lock() {
            println!(
                "DEBUG: Sending to PTY: '{}' (bytes: {:?})",
                text,
                text.as_bytes()
            );
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
            egui::FontData::from_static(d2coding_font_data),
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
                        // Debug: print raw bytes
                        print!("DEBUG PTY: ");
                        for &byte in &buffer[..n] {
                            if byte.is_ascii_graphic() || byte == b' ' {
                                print!("{}", byte as char);
                            } else {
                                print!("\\x{:02x}", byte);
                            }
                        }
                        println!();

                        for &byte in &buffer[..n] {
                            parser.advance(&mut performer, byte);
                        }
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

        println!(
            "DEBUG: Resizing terminal from {:?} to ({}, {})",
            current_size, new_rows, new_cols
        );

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
                ui.label("🖥️ 터미널:");
                ui.label("기본 터미널 기능");
                ui.separator();
                ui.label("🔧 디버그: 콘솔에서 입력 로그 확인");
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
                .id_source("terminal_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // Calculate exact font metrics
                    let font_id = egui::FontId::new(11.0, egui::FontFamily::Monospace);
                    let line_height = ui.fonts(|f| f.row_height(&font_id));
                    let char_width = ui.fonts(|f| f.glyph_width(&font_id, ' '));

                    // Calculate terminal content size
                    if let Ok(state) = self.terminal_state.lock() {
                        let content_height = state.rows as f32 * line_height;
                        let content_width = state.cols as f32 * char_width;

                        // Allocate exact space needed for terminal content
                        let (response, painter) = ui.allocate_painter(
                            egui::Vec2::new(content_width, content_height),
                            egui::Sense::click(),
                        );

                        // Request focus when clicked
                        if response.clicked() {
                            ui.memory_mut(|mem| mem.request_focus(response.id));
                        }

                        // Draw terminal content
                        for (row_idx, row) in state.buffer.iter().enumerate() {
                            let y = response.rect.top() + row_idx as f32 * line_height;
                            let mut col_offset = 0.0;

                            for (col_idx, cell) in row.iter().enumerate() {
                                // Skip continuation markers for wide characters
                                if cell.ch == '\u{0000}' {
                                    continue;
                                }

                                let char_display_width = cell.ch.width().unwrap_or(1);
                                let display_width = char_display_width as f32 * char_width;

                                let x = response.rect.left() + col_offset;
                                let pos = egui::Pos2::new(x, y);

                                // Highlight cursor position
                                if row_idx == state.cursor_row && col_idx == state.cursor_col {
                                    // Show composing Korean character at cursor if any
                                    let display_char = if let Some(composing_char) =
                                        self.korean_state.get_current_char()
                                    {
                                        if self.korean_state.is_composing {
                                            composing_char
                                        } else {
                                            cell.ch
                                        }
                                    } else {
                                        cell.ch
                                    };

                                    // Calculate cursor width - Korean characters need wide cursor
                                    let cursor_width = if self.korean_state.is_composing {
                                        // Korean composing characters are always wide (2 chars)
                                        2.0 * char_width
                                    } else {
                                        // Use actual character width for non-composing
                                        display_width
                                    };

                                    // Different highlight for composing vs normal cursor
                                    let (bg_color, text_color) = if self.korean_state.is_composing {
                                        (egui::Color32::LIGHT_BLUE, egui::Color32::BLACK)
                                    } else {
                                        (egui::Color32::YELLOW, egui::Color32::BLACK)
                                    };

                                    let cursor_rect = egui::Rect::from_min_size(
                                        pos,
                                        egui::Vec2::new(cursor_width, line_height),
                                    );
                                    painter.rect_filled(
                                        cursor_rect,
                                        egui::Rounding::ZERO,
                                        bg_color,
                                    );
                                    painter.text(
                                        pos,
                                        egui::Align2::LEFT_TOP,
                                        display_char,
                                        font_id.clone(),
                                        text_color,
                                    );
                                } else {
                                    painter.text(
                                        pos,
                                        egui::Align2::LEFT_TOP,
                                        cell.ch,
                                        font_id.clone(),
                                        cell.color.foreground,
                                    );
                                }

                                col_offset += display_width;
                            }
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

            // Handle ESC key specially using direct input check
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                println!(
                    "DEBUG: ESC key detected, is_composing: {}",
                    self.korean_state.is_composing
                );
                if self.korean_state.is_composing {
                    // 조합 중이면 조합만 완성하고 ESC는 무시
                    println!("DEBUG: Finalizing Korean composition due to ESC");
                    self.finalize_korean_composition();
                } else {
                    // 조합 중이 아니면 정상적으로 ESC 처리
                    println!("DEBUG: Sending ESC to PTY");
                    self.send_to_pty("\x1b");
                }
                // Force maintain focus after ESC
                ui.memory_mut(|mem| mem.request_focus(terminal_response.inner.id));
            }

            if has_focus {
                ctx.input(|i| {
                    for event in &i.events {
                        match event {
                            egui::Event::Key {
                                key,
                                pressed: true,
                                modifiers,
                                ..
                            } => {
                                println!("DEBUG: Key event received: {:?}, pressed: true", key);

                                // Handle keys that should finalize Korean composition
                                match key {
                                    egui::Key::Enter => {
                                        self.finalize_korean_composition();
                                        if let Ok(mut writer) = self.pty_writer.lock() {
                                            let _ = writer.write_all(b"\n");
                                            let _ = writer.flush();
                                        }
                                    }
                                    egui::Key::Space => {
                                        self.finalize_korean_composition();
                                        self.send_to_pty(" ");
                                    }
                                    egui::Key::Tab => {
                                        self.finalize_korean_composition();
                                        self.send_to_pty("\t");
                                    }

                                    egui::Key::Backspace => {
                                        // Handle backspace for Korean composition
                                        if self.korean_state.is_composing {
                                            // Step-by-step Korean composition backspace
                                            let still_composing =
                                                self.korean_state.handle_backspace();
                                            if !still_composing {
                                                // Composition ended, now do normal backspace
                                                if let Ok(mut writer) = self.pty_writer.lock() {
                                                    let _ = writer.write_all(b"\x7f");
                                                    let _ = writer.flush();
                                                }
                                            }
                                            // If still_composing is true, just update visual without sending to PTY
                                        } else {
                                            // Normal backspace behavior
                                            if let Ok(mut writer) = self.pty_writer.lock() {
                                                let _ = writer.write_all(b"\x7f");
                                                let _ = writer.flush();
                                            }
                                        }
                                    }
                                    egui::Key::ArrowUp => {
                                        if self.korean_state.is_composing {
                                            // 조합 중이면 조합만 완성하고 화살표는 무시
                                            self.finalize_korean_composition();
                                        } else {
                                            // 조합 중이 아니면 정상적으로 화살표 키 처리
                                            self.send_to_pty("\x1b[A");
                                        }
                                    }
                                    egui::Key::ArrowDown => {
                                        if self.korean_state.is_composing {
                                            // 조합 중이면 조합만 완성하고 화살표는 무시
                                            self.finalize_korean_composition();
                                        } else {
                                            // 조합 중이 아니면 정상적으로 화살표 키 처리
                                            self.send_to_pty("\x1b[B");
                                        }
                                    }
                                    egui::Key::ArrowRight => {
                                        if self.korean_state.is_composing {
                                            // 조합 중이면 조합만 완성하고 화살표는 무시
                                            self.finalize_korean_composition();
                                        } else {
                                            // 조합 중이 아니면 정상적으로 화살표 키 처리
                                            self.send_to_pty("\x1b[C");
                                        }
                                    }
                                    egui::Key::ArrowLeft => {
                                        if self.korean_state.is_composing {
                                            // 조합 중이면 조합만 완성하고 화살표는 무시
                                            self.finalize_korean_composition();
                                        } else {
                                            // 조합 중이 아니면 정상적으로 화살표 키 처리
                                            self.send_to_pty("\x1b[D");
                                        }
                                    }
                                    _ => {
                                        // For other keys, handle normally without composition finalization
                                        if let Ok(mut writer) = self.pty_writer.lock() {
                                            match key {
                                                egui::Key::A if modifiers.ctrl => {
                                                    let _ = writer.write_all(b"\x01");
                                                    // Ctrl+A (Start of Heading)
                                                }
                                                egui::Key::C if modifiers.ctrl => {
                                                    let _ = writer.write_all(b"\x03");
                                                    // Ctrl+C
                                                }
                                                egui::Key::D if modifiers.ctrl => {
                                                    let _ = writer.write_all(b"\x04");
                                                    // Ctrl+D
                                                }
                                                egui::Key::E if modifiers.ctrl => {
                                                    let _ = writer.write_all(b"\x05");
                                                    // Ctrl+E (End of Text)
                                                }
                                                egui::Key::L if modifiers.ctrl => {
                                                    let _ = writer.write_all(b"\x0c");
                                                    // Ctrl+L (Form Feed)
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

#[tokio::main]
async fn main() {
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
            // Enable IME support
            cc.egui_ctx.set_debug_on_hover(false);

            Ok(Box::new(
                TerminalApp::new(cc).expect("Failed to create terminal app"),
            ))
        }),
    );
}
