//! German text to sounds for lip sync (docs/LIP_SYNC.md §7.3).
//!
//! Ported from TalkingHead's `lipsync-de.mjs` (https://github.com/met4citizen/TalkingHead),
//! by Stephan Wald, MIT License (full notice in NOTICE). The rule table is the
//! original's, verbatim; `L` is sounded where its rules sound an L (§6.1).

use crate::lipsync_rules::{Classes, RuleLang, Sound};
use crate::lipsync_text::*;

const RULES: &[(&str, &[&str])] = &[
    ("A", &[
        "[AH]=aa", "[AU]=aa U", "[AI]=aa I", "[AE]=E", "[A]H=aa", "[A]U=aa U", "[A]I=aa I",
        " [AN] =aa nn", " [AM] =aa PP", "[ARR]=aa RR", "[AR]=aa RR", " [ALS]=aa nn SS",
        "[AL]=aa nn", "[AUCH]=aa U kk", "[ABER]=aa PP E RR", "[A]=aa",
    ]),
    ("Ä", &[
        "[Ä]H=E", "[ÄU]=O", "[Ä]=E",
    ]),
    ("B", &[
        "[B]=PP",
    ]),
    ("C", &[
        "[CH]S=kk SS", "[CH]=kk", " [CH]=kk", "#[CH]=kk", "[CK]=kk", "[C]H=kk", "[C]=kk",
    ]),
    ("D", &[
        " [DAS] =DD aa SS", " [DEN] =DD E nn", " [DER] =DD E RR", " [DIE] =DD I", " [DU] =DD U",
        " [DURCH]=DD U RR kk", "[D]=DD",
    ]),
    ("E", &[
        "[EI]=aa I", "[EU]=O", "[EH]=E", " [ER] =E RR", " [ES] =E SS", " [EIN] =aa I nn",
        " [EINE]=aa I nn aa", "[ER]#=E", "[ER]=E RR", "[EN]#=aa nn", "[E]=E",
    ]),
    ("F", &[
        "[F]=FF",
    ]),
    ("G", &[
        "[G]=kk",
    ]),
    ("H", &[
        " [HAT] =I aa DD", " [HABEN]=I aa PP aa nn", " [HIER]=I I RR", " [HEUTE]=I O DD aa",
        "[H]=",
    ]),
    ("I", &[
        " [ICH] =I kk", " [IHR] =I RR", " [IN] =I nn", " [IST] =I SS DD", " [IM] =I PP",
        "[IE]=I", "[IH]=I", "[I]=I",
    ]),
    ("J", &[
        "[J]=I",
    ]),
    ("K", &[
        "[K]=kk",
    ]),
    ("L", &[
        "[L]=nn",
    ]),
    ("M", &[
        " [MIT] =PP I DD", " [MAN] =PP aa nn", " [MEHR]=PP E RR", " [MICH]=PP I kk", "[M]=PP",
    ]),
    ("N", &[
        " [NICHT]=nn I kk DD", " [NUR] =nn U RR", " [NACH]=nn aa kk", " [NOCH]=nn aa kk",
        "[NG]=nn kk", "[N]=nn",
    ]),
    ("O", &[
        "[OO]=U", "[OH]=O", "[OU]=aa U", " [ODER]=O DD E RR", " [OHNE]=O nn aa", "[Ö]=E",
        "[O]=aa",
    ]),
    ("Ö", &[
        "[ÖH]=E", "[Ö]=E",
    ]),
    ("P", &[
        "[PF]=FF FF", "[PH]=FF", "[P]=PP",
    ]),
    ("Q", &[
        "[QU]=kk FF", "[Q]=kk",
    ]),
    ("R", &[
        "[R]=RR",
    ]),
    ("S", &[
        "[SCH]=SS", "[SP]=SS PP", "[ST]=SS DD", "[SS]=SS", "[S]=SS",
    ]),
    ("ß", &[
        "[ß]=SS",
    ]),
    ("T", &[
        "[TZ]=DD SS", "[TH]=DD", "[T]=DD",
    ]),
    ("U", &[
        " [UND] =U nn DD", " [UM] =U PP", " [UNTER]=U nn DD E RR", " [UNS] =U nn SS", "[UH]=U",
        "[ÜH]=I U", "[Ü]=I U", "[U]=U",
    ]),
    ("Ü", &[
        "[ÜH]=I U", "[Ü]=I U",
    ]),
    ("V", &[
        " [VON] =FF aa nn", " [VOR] =FF aa RR", " [VIEL]=FF I nn", "[V]=FF",
    ]),
    ("W", &[
        " [WAS] =FF aa SS", " [WIR] =FF I RR", " [WIE] =FF I", " [WENN]=FF E nn",
        " [WILL]=FF I nn", " [WO] =FF aa", " [WIEDER]=FF I DD E RR", "[W]=FF",
    ]),
    ("X", &[
        "[X]=kk SS",
    ]),
    ("Y", &[
        "[Y]=I",
    ]),
    ("Z", &[
        " [ZU] =DD SS U", " [ZUM] =DD SS U PP", " [ZUR] =DD SS U RR", " [ZEIT]=DD SS aa I DD",
        "[Z]=DD SS",
    ]),
];

fn duration(sound: &str) -> f32 {
    match sound {
        "aa" | "U" | "TH" => 1.,
        "E" | "nn" | "L" => 0.85,
        "I" | "RR" => 0.9,
        "O" | "FF" => 1.05,
        "PP" => 1.15,
        "SS" => 1.2,
        "DD" => 1.1,
        "kk" => 1.25,
        _ => 1.,
    }
}

fn pause(c: char) -> f32 {
    match c {
        ' ' => 1.,
        ',' => 3.,
        '-' | '\'' => 0.5,
        '.' => 4.,
        '!' | '?' => 3.,
        _ => 0.,
    }
}

fn lang() -> &'static RuleLang {
    static L: std::sync::OnceLock<RuleLang> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let classes = Classes {
            vowels: "AEIOUÄÖÜ",
            voiced: "BDVGJLMNRWZ",
            suffixes: &["ER", "E", "ES", "ED", "ING", "ELY", "EN", "TE", "ST"],
            sibilant: ("SCGZXJ", &["CH", "SCH", "TZ"]),
            alveolar: ("TSRDLZNJ", &["TH", "CH", "SCH"]),
            consonants: "BCDFGHJKLMNPQRSTVWXYZß",
            consonant_run: false,
            front: "EIY",
        };
        RuleLang::new(RULES, classes, false, true, duration, pause)
    })
}

pub fn word_sounds(word: &str) -> Vec<Sound> {
    lang().sounds(word)
}

const DIGITS: [&str; 10] = ["null", "eins", "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun"];
const ONES: [&str; 10] = ["", "ein", "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun"];
const TENS: [&str; 10] = ["", "", "zwanzig", "dreißig", "vierzig", "fünfzig", "sechzig", "siebzig", "achtzig", "neunzig"];
const TEENS: [&str; 10] = ["zehn", "elf", "zwölf", "dreizehn", "vierzehn", "fünfzehn", "sechzehn", "siebzehn", "achtzehn", "neunzehn"];

fn tens(n: u64) -> String {
    if n < 10 {
        ONES[n as usize].to_string()
    } else if n < 20 {
        TEENS[(n - 10) as usize].to_string()
    } else if n % 10 == 0 {
        TENS[(n / 10) as usize].to_string()
    } else {
        format!("{}und{}", ONES[(n % 10) as usize], TENS[(n / 10) as usize])
    }
}

fn hundreds(n: u64) -> String {
    if n <= 99 {
        return tens(n);
    }
    let mut s = if n / 100 == 1 { "einhundert".to_string() } else { format!("{}hundert", ONES[(n / 100) as usize]) };
    if n % 100 > 0 {
        s += &tens(n % 100);
    }
    s
}

fn thousands(n: u64) -> String {
    if n < 1000 {
        return hundreds(n);
    }
    let mut s = if n / 1000 == 1 { "eintausend".to_string() } else { format!("{}tausend", hundreds(n / 1000)) };
    if n % 1000 > 0 {
        s += &hundreds(n % 1000);
    }
    s
}

fn millions(n: u64) -> String {
    if n < 1_000_000 {
        return thousands(n);
    }
    let m = n / 1_000_000;
    let mut s = thousands(m) + if m == 1 { " million " } else { " millionen " };
    if n % 1_000_000 > 0 {
        s += &thousands(n % 1_000_000);
    }
    s
}

/// A number read aloud (codes and four-digit numbers past 2100 digit by digit).
pub fn number_words(s: &str) -> String {
    if s == "0" {
        return "null".into();
    }
    let digits = || s.chars().filter_map(|d| d.to_digit(10)).map(|d| DIGITS[d as usize]).collect::<Vec<_>>().join(" ");
    let Ok(n) = s.parse::<u64>() else { return digits() };
    if s.starts_with('0') || (s.len() == 4 && n > 2100) {
        return digits();
    }
    millions(n)
}

/// Text ready for the rules (the module's `preProcessText`).
pub fn preprocess(text: &str) -> String {
    let s: String = text.chars().filter(|c| !"#_*\":;".contains(*c)).collect();
    let mut t = String::new();
    for c in s.chars() {
        match c {
            '%' => t += " prozent ",
            '€' => t += " euro ",
            '&' => t += " und ",
            '+' => t += " plus ",
            '$' => t += " dollar ",
            '=' => t += " gleich ",
            '@' => t += " at ",
            c => t.push(c),
        }
    }
    let t = decimal_word(&t, &['.'], "komma");
    let t = decimal_word(&t, &[','], "komma");
    let t = numbers_to_words(&t, &number_words);
    one_space(&squeeze_repeats(&t)).to_lowercase().trim().to_string()
}
