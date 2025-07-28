// 한글 입력 관련 상수
pub const KOREAN_BASE: u32 = 0xAC00;

pub const JUNGSUNG_COUNT: u32 = 21;
pub const JONGSUNG_COUNT: u32 = 28;

// 초성 매핑 (자음 -> 초성 인덱스)
pub fn get_chosung_index(ch: char) -> Option<u32> {
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
pub fn get_jungsung_index(ch: char) -> Option<u32> {
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
pub fn get_jongsung_index(ch: char) -> Option<u32> {
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
pub fn combine_vowels(base: char, add: char) -> Option<char> {
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
pub fn combine_consonants(base: char, add: char) -> Option<char> {
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
pub fn compose_korean(chosung: u32, jungsung: u32, jongsung: u32) -> char {
    let code = KOREAN_BASE + (chosung * JUNGSUNG_COUNT + jungsung) * JONGSUNG_COUNT + jongsung;
    char::from_u32(code).unwrap_or('?')
}

// 자음 여부 확인
pub fn is_consonant(ch: char) -> bool {
    matches!(ch, 'ㄱ'..='ㅎ')
}

// 모음 여부 확인
pub fn is_vowel(ch: char) -> bool {
    matches!(ch, 'ㅏ'..='ㅣ')
}

// 한글 조합 상태 관리
#[derive(Clone, Debug)]
pub struct KoreanInputState {
    pub chosung: Option<char>,  // 초성
    pub jungsung: Option<char>, // 중성
    pub jongsung: Option<char>, // 종성
    pub is_composing: bool,     // 조합 중인지 여부
}

impl KoreanInputState {
    pub fn new() -> Self {
        Self {
            chosung: None,
            jungsung: None,
            jongsung: None,
            is_composing: false,
        }
    }

    pub fn reset(&mut self) {
        self.chosung = None;
        self.jungsung = None;
        self.jongsung = None;
        self.is_composing = false;
    }

    // 현재 조합중인 문자 반환
    pub fn get_current_char(&self) -> Option<char> {
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
    pub fn handle_backspace(&mut self) -> bool {
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
