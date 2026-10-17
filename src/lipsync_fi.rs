//! Finnish and Lithuanian text to sounds for lip sync (docs/LIP_SYNC.md §7.3): both
//! spell as they sound, so each letter maps to one sound.
//!
//! Ported from TalkingHead's `lipsync-fi.mjs` and `lipsync-lt.mjs`
//! (https://github.com/met4citizen/TalkingHead), by Mika Suominen, MIT License (full
//! notice in NOTICE). `L` is sounded for l (§6.1). Fixed from the originals: Finnish
//! reads 20 (it was left out), Lithuanian reads 11–19 as one word ("vienuolika", not
//! "dešimt vienas"), and Lithuanian keeps its carons (š, ž, č), which its accent
//! stripping removed.

use crate::lipsync_rules::{letter_sounds, Sound};
use crate::lipsync_text::*;

// ---------------------------------------------------------------- Finnish

fn fi_letter(c: char) -> Option<(&'static str, f32)> {
    let s = match c {
        'a' | 'ä' => "aa",
        'e' => "E",
        'i' | 'j' => "I",
        'o' | 'ö' | 'å' => "O",
        'u' | 'y' => "U",
        'b' | 'm' | 'p' => "PP",
        'c' | 's' | 'x' | 'z' => "SS",
        'd' | 't' => "DD",
        'f' | 'v' | 'w' => "FF",
        'g' | 'h' | 'k' | 'q' => "kk",
        'l' => "L",
        'n' => "nn",
        'r' => "RR",
        _ => return None,
    };
    let d = match s {
        "aa" | "U" => 0.95,
        "E" => 0.9,
        "I" => 0.92,
        "O" => 0.96,
        "PP" => 1.08,
        "SS" => 1.23,
        "DD" => 1.05,
        "kk" => 1.21,
        "nn" | "RR" | "L" => 0.88,
        _ => 1.,
    };
    Some((s, d))
}

fn fi_pause(c: char) -> f32 {
    match c {
        ' ' => 1.,
        ',' => 3.,
        '-' => 0.5,
        _ => 0.,
    }
}

pub fn fi_word_sounds(word: &str) -> Vec<Sound> {
    letter_sounds(word, fi_letter, fi_pause)
}

const FI_NUMBERS: [&str; 20] = [
    "nolla", "yksi", "kaksi", "kolme", "neljä", "viisi", "kuusi", "seitsemän", "kahdeksan", "yhdeksän", "kymmenen", "yksitoista", "kaksitoista",
    "kolmetoista", "neljätoista", "viisitoista", "kuusitoista", "seitsemäntoista", "kahdeksantoista", "yhdeksäntoista",
];

/// An integer read aloud in Finnish.
pub fn fi_number_words(x: &str) -> String {
    let Ok(mut n) = x.parse::<u64>() else { return x.to_string() };
    let mut w: Vec<String> = vec![];
    let part = |n: u64, z: u64, w0: &str, w1: &str, w2: &str, w: &mut Vec<String>| -> u64 {
        if n < z {
            return n;
        }
        let d = n / z;
        w.push(format!("{w0}{}", if d == 1 { w1.to_string() } else { fi_number_words(&d.to_string()) + w2 }));
        n - d * z
    };
    n = part(n, 1_000_000_000, " ", "miljardi", " miljardia", &mut w);
    n = part(n, 1_000_000, " ", "miljoona", " miljoonaa", &mut w);
    n = part(n, 1000, "", "tuhat", "tuhatta", &mut w);
    n = part(n, 100, " ", "sata", "sataa", &mut w);
    if n >= 20 {
        n = part(n, 10, "", "", "kymmentä", &mut w);
    }
    if n >= 1 {
        w.push(FI_NUMBERS[n as usize].into());
    }
    if x.parse::<u64>() == Ok(0) {
        w.push("nolla".into());
    }
    w.join("").trim().to_string()
}

/// Accents other than Finnish's own (ä, ö, å) dropped.
fn fi_letters(s: &str) -> String {
    s.chars().map(|c| if "äöåÄÖÅ".contains(c) { c } else { base_letter(c).unwrap_or(c) }).collect()
}

/// Text ready for the letters (the module's `preProcessText`).
pub fn fi_preprocess(text: &str) -> String {
    let s: String = text.chars().filter(|c| !"#_*'\":;".contains(*c)).collect();
    let mut t = String::new();
    for c in s.chars() {
        match c {
            '%' => t += " prosenttia ",
            '€' => t += " euroa ",
            '&' => t += " ja ",
            '+' => t += " plus ",
            '$' => t += " dollaria ",
            c => t.push(c),
        }
    }
    let t = decimal_word(&t, &[','], "pilkku");
    let t = numbers_to_words(&t, &fi_number_words);
    fi_letters(&one_space(&t)).trim().to_string()
}

// ---------------------------------------------------------------- Lithuanian

fn lt_letter(c: char) -> Option<(&'static str, f32)> {
    Some(match c {
        'a' => ("aa", 0.95),
        'ą' => ("O", 1.5),
        'e' => ("E", 0.9),
        'ę' | 'ė' => ("E", 1.5),
        'i' => ("I", 0.92),
        'į' | 'y' => ("I", 1.5),
        'o' => ("O", 0.96),
        'u' => ("U", 0.95),
        'ū' | 'ų' => ("U", 1.5),
        'b' | 'm' | 'p' => ("PP", 1.08),
        'c' | 'č' | 's' | 'x' | 'z' | 'ž' => ("SS", 1.23),
        'š' => ("CH", 1.23),
        'd' | 't' => ("DD", 1.05),
        'f' | 'v' | 'w' => ("FF", 1.),
        'g' | 'h' | 'k' | 'q' => ("kk", 1.21),
        'j' => ("I", 0.92),
        'l' => ("L", 0.88),
        'n' => ("nn", 0.88),
        'r' => ("RR", 0.88),
        _ => return None,
    })
}

pub fn lt_word_sounds(word: &str) -> Vec<Sound> {
    letter_sounds(word, lt_letter, fi_pause)
}

const LT_NUMBERS: [&str; 20] = [
    "nulis", "vienas", "du", "trys", "keturi", "penki", "šeši", "septyni", "aštuoni", "devyni", "dešimt", "vienuolika", "dvylika", "trylika",
    "keturiolika", "penkiolika", "šešiolika", "septyniolika", "aštuoniolika", "devyniolika",
];
const LT_TENS: [&str; 10] = [
    "nulis", "dešimt", "dvidešimt", "trisdešimt", "keturiasdešimt", "penkiasdešimt", "šešiasdešimt", "septyniasdešimt", "aštuoniasdešimt", "devyniasdešimt",
];

/// An integer read aloud in Lithuanian, with the plural forms of thousands etc.
pub fn lt_number_words(x: &str) -> String {
    let Ok(mut n) = x.parse::<u64>() else { return x.to_string() };
    let mut w: Vec<String> = vec![];
    let part = |n: u64, z: u64, one: &str, few: &str, many: &str, w: &mut Vec<String>| -> u64 {
        if n < z {
            return n;
        }
        let d = n / z;
        w.push(if d == 1 { LT_NUMBERS[1].to_string() } else { lt_number_words(&d.to_string()) });
        w.push(if d % 10 == 1 { one } else if d % 10 == 0 || (d % 100 > 10 && d % 100 < 20) { many } else { few }.into());
        n - d * z
    };
    n = part(n, 1_000_000_000, "milijardas", "milijardai", "milijardų", &mut w);
    n = part(n, 1_000_000, "milijonas", "milijonai", "milijonų", &mut w);
    n = part(n, 1000, "tūkstantis", "tūkstančiai", "tūkstančių", &mut w);
    n = part(n, 100, "šimtas", "šimtai", "šimtų", &mut w);
    if n >= 20 {
        w.push(LT_TENS[(n / 10) as usize].into());
        n %= 10;
    }
    if n >= 1 {
        w.push(LT_NUMBERS[n as usize].into());
    }
    if x.parse::<u64>() == Ok(0) {
        w.push(LT_NUMBERS[0].into());
    }
    w.join(" ").trim().to_string()
}

/// Accents other than Lithuanian's own dropped.
fn lt_letters(s: &str) -> String {
    s.chars().map(|c| if "ąčęėįšųūžĄČĘĖĮŠŲŪŽ".contains(c) { c } else { base_letter(c).unwrap_or(c) }).collect()
}

/// Text ready for the letters (the module's `preProcessText`).
pub fn lt_preprocess(text: &str) -> String {
    let mut s = text.to_string();
    for (from, to) in [
        ("0 %", "0 procentų "),
        ("1 %", "1 procentas "),
        ("%", " procentai "),
        ("0 €", "0 eurų "),
        ("1 €", "1 euras "),
        ("€", " eurai "),
        ("0 $", "0 dolerių "),
        ("1 $", "1 doleris "),
        ("$", " doleriai "),
        ("&", " ir "),
        ("+", " pliusas "),
    ] {
        s = s.replace(from, to);
    }
    let s = decimal_word(&s, &[','], "kablelis");
    let s = numbers_to_words(&s, &lt_number_words);
    lt_letters(&one_space(&squeeze_repeats(&s))).trim().to_string()
}
