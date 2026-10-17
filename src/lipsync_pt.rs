//! Brazilian Portuguese text to sounds for lip sync (docs/LIP_SYNC.md §7.3).
//!
//! Ported from TalkingHead's `lipsync-pt_br.mjs` (https://github.com/met4citizen/TalkingHead),
//! by Lucas Boemeke, MIT License (full notice in NOTICE): a letter table with context
//! rules for digraphs, soft and hard c and g, silent h and u, affricates, the vocalised
//! coda l, nasal codas and final vowels. `L` is sounded for an l before a vowel (§6.1).

use crate::lipsync_rules::Sound;
use crate::lipsync_text::*;

fn letter(c: char) -> Option<&'static str> {
    Some(match c {
        'a' | 'á' | 'à' | 'â' | 'ã' => "aa",
        'e' | 'é' | 'ê' => "E",
        'i' | 'í' | 'y' => "I",
        'o' | 'ó' | 'ô' | 'õ' => "O",
        'u' | 'ú' | 'ü' | 'w' => "U",
        'b' | 'p' | 'm' => "PP",
        'f' | 'v' => "FF",
        'd' | 't' => "DD",
        'c' | 'k' | 'g' | 'q' => "kk",
        'j' | 'x' => "CH",
        's' | 'z' | 'ç' => "SS",
        'n' => "nn",
        'l' => "L",
        'r' => "RR",
        _ => return None,
    })
}

fn duration(sound: &str) -> f32 {
    match sound {
        "aa" | "U" => 0.95,
        "E" => 0.9,
        "I" => 0.92,
        "O" => 0.96,
        "PP" => 1.08,
        "SS" => 1.23,
        "DD" => 1.05,
        "FF" => 1.,
        "kk" => 1.21,
        "nn" | "RR" | "L" => 0.88,
        "CH" => 1.15,
        _ => 1.,
    }
}

fn pause(c: char) -> f32 {
    match c {
        ' ' => 1.,
        ',' => 3.,
        '-' => 0.5,
        _ => 0.,
    }
}

const VOWELS: &str = "aáàâãeéêiíoóôõuúüy";
const FRONT: &str = "eéêiíy";

/// A word's sounds (the module's `wordsToVisemes`).
pub fn word_sounds(word: &str) -> Vec<Sound> {
    let mut out: Vec<Sound> = vec![];
    let mut t = 0.;
    let mut push = |s: &'static str, t: &mut f32| match out.last_mut() {
        Some(last) if last.0 == s => {
            let d = 0.7 * duration(s);
            last.2 += d;
            *t += d;
        }
        _ => {
            let d = duration(s);
            out.push((s, *t, d));
            *t += d;
        }
    };
    let chars: Vec<char> = word.to_lowercase().chars().collect();
    let at = |i: isize| if i < 0 { None } else { chars.get(i as usize).copied() };
    let is_letter = |c: Option<char>| c.is_some_and(char::is_alphabetic);
    let is_vowel = |c: Option<char>| c.is_some_and(|c| VOWELS.contains(c));
    let is_front = |c: Option<char>| c.is_some_and(|c| FRONT.contains(c));
    let is_end = |i: isize| !is_letter(at(i));
    let is_final = |i: isize| is_end(i) || (at(i) == Some('s') && is_end(i + 1));

    let mut i: isize = 0;
    while (i as usize) < chars.len() {
        let c = chars[i as usize];
        let (c1, c2) = (at(i + 1), at(i + 2));
        let start = !is_letter(at(i - 1));

        // digraphs
        if c == 'c' && c1 == Some('h') {
            push("CH", &mut t);
            i += 2;
            continue;
        }
        if (c == 'l' || c == 'n') && c1 == Some('h') {
            push("nn", &mut t);
            i += 2;
            continue;
        }
        if (c == 'r' || c == 's') && c1 == Some(c) {
            push(if c == 'r' { "RR" } else { "SS" }, &mut t);
            i += 2;
            continue;
        }
        if (c == 's' || c == 'x') && c1 == Some('c') && is_front(c2) {
            push("SS", &mut t);
            i += 2;
            continue;
        }
        // consonants with context
        match c {
            'q' => {
                push("kk", &mut t);
                i += if c1 == Some('u') && is_front(c2) { 2 } else { 1 };
                continue;
            }
            'g' => {
                if c1 == Some('u') && is_front(c2) {
                    push("kk", &mut t);
                    i += 2;
                } else {
                    push(if is_front(c1) { "CH" } else { "kk" }, &mut t);
                    i += 1;
                }
                continue;
            }
            'c' => {
                push(if is_front(c1) { "SS" } else { "kk" }, &mut t);
                i += 1;
                continue;
            }
            'h' => {
                i += 1;
                continue;
            }
            'x' => {
                let s = (is_letter(c1) && !is_vowel(c1)) || (matches!(at(i - 1), Some('e') | Some('ê')) && !is_letter(at(i - 2)) && is_vowel(c1));
                push(if s { "SS" } else { "CH" }, &mut t);
                i += 1;
                continue;
            }
            't' | 'd' => {
                let affricate = matches!(c1, Some('i') | Some('í')) || (c1 == Some('e') && is_final(i + 2));
                push(if affricate { "CH" } else { "DD" }, &mut t);
                i += 1;
                continue;
            }
            'l' => {
                push(if is_vowel(c1) { "L" } else { "U" }, &mut t);
                i += 1;
                continue;
            }
            'm' | 'n' => {
                if is_vowel(c1) {
                    push(if c == 'm' { "PP" } else { "nn" }, &mut t);
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        // nasal diphthongs and final vowels
        if c == 'ã' && c1 == Some('o') {
            push("aa", &mut t);
            push("U", &mut t);
            i += 2;
            continue;
        }
        if c == 'a' && c1 == Some('m') && is_end(i + 2) {
            push("aa", &mut t);
            push("U", &mut t);
            i += 2;
            continue;
        }
        if (c == 'e' || c == 'é') && ((c1 == Some('m') && is_end(i + 2)) || (c1 == Some('n') && c2 == Some('s') && is_end(i + 3))) {
            push("E", &mut t);
            push("I", &mut t);
            i += 2;
            continue;
        }
        if c == 'o' && c1 == Some('u') {
            push("O", &mut t);
            i += 2;
            continue;
        }
        if c == 'e' && ((is_final(i + 1) && !start) || (start && is_end(i + 1))) {
            push("I", &mut t);
            i += 1;
            continue;
        }
        if c == 'o' && is_final(i + 1) {
            push("U", &mut t);
            i += 1;
            continue;
        }
        // the letter table, falling back to the base letter of unknown accents
        let v = letter(c).or_else(|| if c.is_alphabetic() { base_letter(c).and_then(letter) } else { None });
        match v {
            Some(v) => push(v, &mut t),
            None => t += pause(c),
        }
        i += 1;
    }
    out
}

const UNITS: [&str; 20] = [
    "zero", "um", "dois", "três", "quatro", "cinco", "seis", "sete", "oito", "nove", "dez", "onze", "doze", "treze", "quatorze", "quinze", "dezesseis",
    "dezessete", "dezoito", "dezenove",
];
const TENS: [&str; 10] = ["", "", "vinte", "trinta", "quarenta", "cinquenta", "sessenta", "setenta", "oitenta", "noventa"];
const HUNDREDS: [&str; 10] = ["", "cento", "duzentos", "trezentos", "quatrocentos", "quinhentos", "seiscentos", "setecentos", "oitocentos", "novecentos"];

fn below_1000(n: u64) -> String {
    if n == 100 {
        return "cem".into();
    }
    let mut parts: Vec<String> = vec![];
    let (h, r) = (n / 100, n % 100);
    if h > 0 {
        parts.push(HUNDREDS[h as usize].into());
    }
    if r < 20 {
        if r > 0 {
            parts.push(UNITS[r as usize].into());
        }
    } else {
        let (t, u) = (r / 10, r % 10);
        parts.push(if u > 0 { format!("{} e {}", TENS[t as usize], UNITS[u as usize]) } else { TENS[t as usize].into() });
    }
    parts.join(" e ")
}

/// An integer read aloud (codes with a leading zero, and very long numbers, digit by
/// digit).
pub fn number_words(x: &str) -> String {
    let Ok(n) = x.parse::<u64>() else { return x.to_string() };
    if x.len() > 12 || (x.len() > 1 && x.starts_with('0')) {
        return x.chars().filter_map(|d| d.to_digit(10)).map(|d| UNITS[d as usize]).collect::<Vec<_>>().join(" ");
    }
    if n == 0 {
        return "zero".into();
    }
    let mut s = String::new();
    let mut add = |g: u64, w: String| {
        if g > 0 {
            if !s.is_empty() {
                s += if g < 100 || g % 100 == 0 { " e " } else { " " };
            }
            s += &w;
        }
    };
    let (bi, mi, th, un) = (n / 1_000_000_000, (n / 1_000_000) % 1000, (n / 1000) % 1000, n % 1000);
    add(bi, if bi == 1 { "um bilhão".into() } else { format!("{} bilhões", below_1000(bi)) });
    add(mi, if mi == 1 { "um milhão".into() } else { format!("{} milhões", below_1000(mi)) });
    add(th, if th == 1 { "mil".into() } else { format!("{} mil", below_1000(th)) });
    add(un, below_1000(un));
    s
}

/// `R$10`, `$ 10`, `10€` → `10 reais` etc. (the module's currency expression).
fn currencies(s: &str) -> String {
    const SYMS: [(&str, &str, &str); 4] = [("R$", "real", "reais"), ("US$", "dólar", "dólares"), ("$", "dólar", "dólares"), ("€", "euro", "euros")];
    let c: Vec<char> = s.chars().collect();
    let sym_at = |i: usize| SYMS.iter().find(|(sym, _, _)| c[i..].starts_with(&sym.chars().collect::<Vec<_>>()[..]));
    // a number: digits, then groups of [.,] and digits
    let num_at = |i: usize| -> Option<usize> {
        let mut j = i;
        while j < c.len() && c[j].is_ascii_digit() {
            j += 1;
        }
        if j == i {
            return None;
        }
        while j + 1 < c.len() && (c[j] == '.' || c[j] == ',') && c[j + 1].is_ascii_digit() {
            j += 1;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
        }
        Some(j)
    };
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        // symbol, spaces, number
        if let Some((sym, one, many)) = sym_at(i) {
            let mut j = i + sym.chars().count();
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if let Some(e) = num_at(j) {
                let num: String = c[j..e].iter().collect();
                out += &format!(" {num} {} ", if num == "1" { one } else { many });
                i = e;
                continue;
            }
        }
        // number, spaces, symbol
        if let Some(e) = num_at(i) {
            let mut j = e;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if let Some((sym, one, many)) = (j < c.len()).then(|| sym_at(j)).flatten() {
                let num: String = c[i..e].iter().collect();
                out += &format!(" {num} {} ", if num == "1" { one } else { many });
                i = j + sym.chars().count();
                continue;
            }
            // not a currency: copy the number as is
            out.extend(&c[i..e]);
            i = e;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Text ready for the rules (the module's `preProcessText`).
pub fn preprocess(text: &str) -> String {
    let s: String = text.chars().filter(|c| !"#_*'\":;".contains(*c)).collect();
    let s = currencies(&s);
    let mut t = String::new();
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let rest: String = c[i..c.len().min(i + 3)].iter().collect();
        if rest.starts_with("R$") {
            t += " reais ";
            i += 2;
            continue;
        }
        if rest.starts_with("US$") {
            t += " dólares ";
            i += 3;
            continue;
        }
        match c[i] {
            '%' => t += " por cento ",
            '€' => t += " euros ",
            '&' => t += " e ",
            '+' => t += " mais ",
            '$' => t += " dólares ",
            ch => t.push(ch),
        }
        i += 1;
    }
    // thousands separator: 1.234 → 1234 (a dot between a digit and exactly three digits)
    let c: Vec<char> = t.chars().collect();
    let mut u = String::new();
    for (i, ch) in c.iter().enumerate() {
        let thousands = *ch == '.'
            && i > 0
            && c[i - 1].is_ascii_digit()
            && i + 4 <= c.len()
            && c[i + 1..i + 4].iter().all(char::is_ascii_digit)
            && !c.get(i + 4).is_some_and(char::is_ascii_digit);
        if !thousands {
            u.push(*ch);
        }
    }
    let u = decimal_word(&u, &[','], "vírgula");
    let u = numbers_to_words(&u, &number_words);
    one_space(&u).trim().to_string()
}
