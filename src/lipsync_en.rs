//! English text to sounds for lip sync (docs/LIP_SYNC.md §7.3).
//!
//! Ported from TalkingHead's `lipsync-en.mjs` (https://github.com/met4citizen/TalkingHead),
//! MIT License, Copyright (c) 2023-2024 Mika Suominen (full notice in NOTICE). Its rules are adapted from NRL
//! Report 7948, "Automatic Translation of English Text to Phonetics by Means of
//! Letter-to-Sound Rules" (Elovitz, Johnson, McHugh, Shore, 1976).
//!
//! Changes from the original: the rules emit `L` where the letter L is sounded (Oculus
//! folds it into `nn`) and `W` for the w sound (the original writes `FF`), as Preston
//! Blair draws both apart; `OO` and `EE`, which aren't visemes, are `U` and `E`.

pub use crate::lipsync_rules::Sound;
use crate::lipsync_rules::{Classes, RuleLang};

/// Rules for each letter, tried in order: `left[letters]right=sounds`. Context symbols:
/// `#` one or more vowels, `.` a voiced consonant, `%` a suffix (ER E ES ED ING ELY),
/// `&` a sibilant, `@` T S R D L Z N J TH CH SH, `^` one consonant, `+` E I or Y,
/// `:` zero or more consonants, and a space for the word's edge.
const RULES: &[(&str, &[&str])] = &[
    ("A", &[
        "[A] =aa", " [ARE] =aa RR", " [AR]O=aa RR", "[AR]#=E RR",
        " ^[AS]#=E SS", "[A]WA=aa", "[AW]=aa", " :[ANY]=E nn I",
        "[A]^+#=E", "#:[ALLY]=aa L I", " [AL]#=aa L", "[AGAIN]=aa kk E nn",
        "#:[AG]E=I kk", "[A]^+:#=aa", ":[A]^+ =E", "[A]^%=E",
        " [ARR]=aa RR", "[ARR]=aa RR", " :[AR] =aa RR", "[AR] =E",
        "[AR]=aa RR", "[AIR]=E RR", "[AI]=E", "[AY]=E", "[AU]=aa",
        "#:[AL] =aa L", "#:[ALS] =aa L SS", "[ALK]=aa kk", "[AL]^=aa L",
        " :[ABLE]=E PP aa L", "[ABLE]=aa PP aa L", "[ANG]+=E nn kk", "[A]=aa",
    ]),
    ("B", &[" [BE]^#=PP I", "[BEING]=PP I I nn", " [BOTH] =PP O TH", " [BUS]#=PP I SS", "[BUIL]=PP I L", "[B]=PP"]),
    ("C", &[" [CH]^=kk", "^E[CH]=kk", "[CH]=CH", " S[CI]#=SS aa", "[CI]A=SS", "[CI]O=SS", "[CI]EN=SS", "[C]+=SS", "[CK]=kk", "[COM]%=kk aa PP", "[C]=kk"]),
    ("D", &["#:[DED] =DD I DD", ".E[D] =DD", "#^:E[D] =DD", " [DE]^#=DD I", " [DO] =DD U", " [DOES]=DD aa SS", " [DOING]=DD U I nn", " [DOW]=DD aa", "[DU]A=kk U", "[D]=DD"]),
    ("E", &[
        "#:[E] =", "'^:[E] =", " :[E] =I", "#[ED] =DD", "#:[E]D =",
        "[EV]ER=E FF", "[E]^%=I", "[ERI]#=I RR I", "[ERI]=E RR I",
        "#:[ER]#=E", "[ER]#=E RR", "[ER]=E", " [EVEN]=I FF E nn",
        "#:[E]W=", "@[EW]=U", "[EW]=I U", "[E]O=I", "#:&[ES] =I SS",
        "#:[E]S =", "#:[ELY] =L I", "#:[EMENT]=PP E nn DD", "[EFUL]=FF U L",
        "[EE]=I", "[EARN]=E nn", " [EAR]^=E", "[EAD]=E DD", "#:[EA] =I aa",
        "[EA]SU=E", "[EA]=I", "[EIGH]=E", "[EI]=I", " [EYE]=aa", "[EY]=I",
        "[EU]=I U", "[E]=E",
    ]),
    ("F", &["[FUL]=FF U L", "[F]=FF"]),
    ("G", &["[GIV]=kk I FF", " [G]I^=kk", "[GE]T=kk E", "SU[GGES]=kk kk E SS", "[GG]=kk", " B#[G]=kk", "[G]+=kk", "[GREAT]=kk RR E DD", "#[GH]=", "[G]=kk"]),
    ("H", &[" [HAV]=I aa FF", " [HERE]=I I RR", " [HOUR]=aa E", "[HOW]=I aa", "[H]#=I", "[H]="]),
    ("I", &[
        " [IN]=I nn", " [I] =aa", "[IN]D=aa nn", "[IER]=I E",
        "#:R[IED] =I DD", "[IED] =aa DD", "[IEN]=I E nn", "[IE]T=aa E",
        " :[I]%=aa", "[I]%=I", "[IE]=I", "[I]^+:#=I", "[IR]#=aa RR",
        "[IZ]%=aa SS", "[IS]%=aa SS", "[I]D%=aa", "+^[I]^+=I",
        "[I]T%=aa", "#^:[I]^+=I", "[I]^+=aa", "[IR]=E", "[IGH]=aa",
        "[ILD]=aa L DD", "[IGN] =aa nn", "[IGN]^=aa nn", "[IGN]%=aa nn",
        "[IQUE]=I kk", "[I]=I",
    ]),
    ("J", &["[J]=kk"]),
    ("K", &[" [K]N=", "[K]=kk"]),
    ("L", &["[LO]C#=L O", "L[L]=", "#^:[L]%=aa L", "[LEAD]=L I DD", "[L]=L"]),
    ("M", &["[MOV]=PP U FF", "[M]=PP"]),
    ("N", &["E[NG]+=nn kk", "[NG]R=nn kk", "[NG]#=nn kk", "[NGL]%=nn kk aa L", "[NG]=nn", "[NK]=nn kk", " [NOW] =nn aa", "[N]=nn"]),
    ("O", &[
        "[OF] =aa FF", "[OROUGH]=E O", "#:[OR] =E", "#:[ORS] =E SS",
        "[OR]=aa RR", " [ONE]=W aa nn", "[OW]=O", " [OVER]=O FF E",
        "[OV]=aa FF", "[O]^%=O", "[O]^EN=O", "[O]^I#=O", "[OL]D=O L",
        "[OUGHT]=aa DD", "[OUGH]=aa FF", " [OU]=aa", "H[OU]S#=aa",
        "[OUS]=aa SS", "[OUR]=aa RR", "[OULD]=U DD", "^[OU]^L=aa",
        "[OUP]=U", "[OU]=aa", "[OY]=O", "[OING]=O I nn", "[OI]=O",
        "[OOR]=aa RR", "[OOK]=U kk", "[OOD]=U DD", "[OO]=U", "[O]E=O",
        "[O] =O", "[OA]=O", " [ONLY]=O nn L I", " [ONCE]=W aa nn SS",
        "[ON'T]=O nn DD", "C[O]N=aa", "[O]NG=aa", " ^:[O]N=aa",
        "I[ON]=aa nn", "#:[ON] =aa nn", "#^[ON]=aa nn", "[O]ST =O",
        "[OF]^=aa FF", "[OTHER]=aa TH E", "[OSS] =aa SS", "#^:[OM]=aa PP",
        "[O]=aa",
    ]),
    ("P", &["[PH]=FF", "[PEOP]=PP I PP", "[POW]=PP aa", "[PUT] =PP U DD", "[P]=PP"]),
    ("Q", &["[QUAR]=kk W aa RR", "[QU]=kk W", "[Q]=kk"]),
    ("R", &[" [RE]^#=RR I", "[R]=RR"]),
    ("S", &[
        "[SH]=SS", "#[SION]=SS aa nn", "[SOME]=SS aa PP", "#[SUR]#=SS E",
        "[SUR]#=SS E", "#[SU]#=SS U", "#[SSU]#=SS U", "#[SED] =SS DD",
        "#[S]#=SS", "[SAID]=SS E DD", "^[SION]=SS aa nn", "[S]S=",
        ".[S] =SS", "#:.E[S] =SS", "#^:##[S] =SS", "#^:#[S] =SS",
        "U[S] =SS", " :#[S] =SS", " [SCH]=SS kk", "[S]C+=",
        "#[SM]=SS PP", "#[SN]'=SS aa nn", "[S]=SS",
    ]),
    ("T", &[
        " [THE] =TH aa", "[TO] =DD U", "[THAT] =TH aa DD", " [THIS] =TH I SS",
        " [THEY]=TH E", " [THERE]=TH E RR", "[THER]=TH E", "[THEIR]=TH E RR",
        " [THAN] =TH aa nn", " [THEM] =TH E PP", "[THESE] =TH I SS",
        " [THEN]=TH E nn", "[THROUGH]=TH RR U", "[THOSE]=TH O SS",
        "[THOUGH] =TH O", " [THUS]=TH aa SS", "[TH]=TH", "#:[TED] =DD I DD",
        "S[TI]#N=CH", "[TI]O=SS", "[TI]A=SS", "[TIEN]=SS aa nn",
        "[TUR]#=CH E", "[TU]A=CH U", " [TWO]=DD U", "[T]=DD",
    ]),
    ("U", &[
        " [UN]I=I U nn", " [UN]=aa nn", " [UPON]=aa PP aa nn",
        "@[UR]#=U RR", "[UR]#=I U RR", "[UR]=E", "[U]^ =aa",
        "[U]^^=aa", "[UY]=aa", " G[U]#=", "G[U]%=", "G[U]#=W",
        "#N[U]=I U", "@[U]=I", "[U]=I U",
    ]),
    ("V", &["[VIEW]=FF I U", "[V]=FF"]),
    ("W", &[" [WERE]=W E", "[WA]S=W aa", "[WA]T=W aa", "[WHERE]=W E RR", "[WHAT]=W aa DD", "[WHOL]=I O L", "[WHO]=I U", "[WH]=W", "[WAR]=W aa RR", "[WOR]^=W E", "[WR]=RR", "[W]=W"]),
    ("X", &[" [X]=SS", "[X]=kk SS"]),
    ("Y", &["[YOUNG]=I aa nn", " [YOU]=I U", " [YES]=I E SS", " [Y]=I", "#^:[Y] =I", "#^:[Y]I=I", " :[Y] =aa", " :[Y]#=aa", " :[Y]^+:#=I", " :[Y]^#=I", "[Y]=I"]),
    ("Z", &["[Z]=SS"]),
];

/// Each sound's relative length (TalkingHead's; L and W as their Oculus folds).
pub fn duration(sound: &str) -> f32 {
    match sound {
        "aa" => 0.95,
        "E" => 0.90,
        "I" => 0.92,
        "O" => 0.96,
        "U" | "W" => 0.95,
        "PP" => 1.08,
        "SS" => 1.23,
        "DD" => 1.05,
        "kk" => 1.21,
        "nn" | "RR" | "L" => 0.88,
        _ => 1.,
    }
}

fn pause(c: char) -> f32 {
    match c {
        ' ' => 1.,
        ',' => 3.,
        '-' | '\'' => 0.5,
        _ => 0.,
    }
}

fn lang() -> &'static RuleLang {
    static L: std::sync::OnceLock<RuleLang> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let classes = Classes {
            vowels: "AEIOUY",
            voiced: "BDVGJLMNRWZ",
            suffixes: &["ER", "E", "ES", "ED", "ING", "ELY"],
            sibilant: ("SCGZXJ", &["CH", "SH"]),
            alveolar: ("TSRDLZNJ", &["TH", "CH", "SH"]),
            consonants: "BCDFGHJKLMNPQRSTVWXZ",
            consonant_run: false,
            front: "EIY",
        };
        // the table already sounds L and W apart
        RuleLang::new(RULES, classes, false, false, duration, pause)
    })
}

/// A word's sounds, with relative starts and lengths (TalkingHead's `wordsToVisemes`).
/// A sound repeated straight after itself lengthens the first instead.
pub fn word_sounds(word: &str) -> Vec<Sound> {
    lang().sounds(word)
}

// ---------------------------------------------------------------- numbers and symbols

const ONES: [&str; 10] = ["", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
const TEENS: [&str; 10] = ["ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const DIGITS: [&str; 10] = ["oh", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];

fn digit_by_digit(s: &str) -> String {
    s.bytes().filter(u8::is_ascii_digit).map(|d| DIGITS[(d - b'0') as usize]).collect::<Vec<_>>().join(" ")
}

fn tens(n: u64, s: &str) -> String {
    if n < 10 {
        let oh = if n != 0 && s.starts_with('0') { "oh " } else { "" };
        format!("{oh}{}", ONES[n as usize])
    } else if n < 20 {
        TEENS[(n - 10) as usize].to_string()
    } else {
        format!("{} {}", TENS[(n / 10) as usize], ONES[(n % 10) as usize]).trim().to_string()
    }
}

fn hundreds(n: u64) -> String {
    if n > 99 { format!("{} hundred {}", ONES[(n / 100) as usize], tens(n % 100, "")) } else { tens(n, "") }
}

fn thousands(n: u64) -> String {
    if n >= 1000 { format!("{} thousand {}", hundreds(n / 1000), hundreds(n % 1000)) } else { hundreds(n) }
}

fn millions(n: u64) -> String {
    if n >= 1_000_000 { format!("{} million {}", millions(n / 1_000_000), thousands(n % 1_000_000)) } else { thousands(n) }
}

/// A number read aloud, deciding how (TalkingHead's `convertNumberToWords`): codes
/// digit by digit, years in pairs.
pub fn number_words(s: &str, plain: bool) -> String {
    if s == "0" {
        return "zero".into();
    }
    if let Some(rest) = s.strip_prefix('-') {
        return format!("minus {}", number_words(rest, plain));
    }
    if let Some((int, frac)) = s.split_once('.') {
        return format!("{} point {}", number_words(if int.is_empty() { "0" } else { int }, true), digit_by_digit(frac));
    }
    if s.starts_with('0') {
        return digit_by_digit(s);
    }
    let Ok(n) = s.parse::<u64>() else { return digit_by_digit(s) };
    let words = if !plain && ((n < 1000 && n > 99 && n % 100 != 0) || (n > 10000 && n < 1_000_000)) {
        digit_by_digit(s)
    } else if !plain && ((n > 1000 && n < 2000) || (n > 2009 && n < 3000)) {
        if n % 100 != 0 {
            format!("{} {}", tens(n / 100, &s[..2]), tens(n % 100, &s[2..]))
        } else {
            format!("{} hundred", tens(n / 100, ""))
        }
    } else {
        millions(n)
    };
    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ordinal(n: u64) -> String {
    const SMALL: [&str; 20] = [
        "", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth", "eleventh", "twelfth",
        "thirteenth", "fourteenth", "fifteenth", "sixteenth", "seventeenth", "eighteenth", "nineteenth",
    ];
    const TENTHS: [&str; 10] = ["", "", "twentieth", "thirtieth", "fortieth", "fiftieth", "sixtieth", "seventieth", "eightieth", "ninetieth"];
    if n < 20 {
        return SMALL[n as usize].to_string();
    }
    if n < 100 && n % 10 == 0 {
        return TENTHS[(n / 10) as usize].to_string();
    }
    let (h, t, o) = (n / 100, (n % 100) / 10 * 10, n % 10);
    let mut s: Vec<String> = vec![];
    if h > 0 {
        s.push(number_words(&h.to_string(), true));
        s.push(if t > 0 || o > 0 { "hundred" } else { "hundredth" }.into());
    }
    let rest = n % 100;
    if rest > 0 && rest < 20 {
        s.push(SMALL[rest as usize].into());
    } else {
        if t > 0 {
            s.push(if o > 0 { TENS[(t / 10) as usize].to_string() } else { TENTHS[(t / 10) as usize].to_string() });
        }
        if o > 0 {
            s.push(SMALL[o as usize].into());
        }
    }
    s.join(" ")
}

/// Text ready for the rules: symbols and numbers spelled out, other characters that
/// aren't spoken dropped (TalkingHead's `preProcessText`, without its decades).
pub fn preprocess(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() || (c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) && (i == 0 || !chars[i - 1].is_alphanumeric())) {
            // a number: digits, thousands commas, one decimal point
            let start = i;
            i += 1;
            while i < chars.len() {
                let d = chars[i];
                let next_digit = chars.get(i + 1).is_some_and(|x| x.is_ascii_digit());
                if d.is_ascii_digit() || ((d == ',' || d == '.') && next_digit) {
                    i += 1;
                } else {
                    break;
                }
            }
            let raw: String = chars[start..i].iter().collect();
            let plain = raw.contains(',') || raw.contains('.');
            let num = raw.replace(',', "");
            // ordinals: 1st, 22nd
            let suffix: String = chars[i..].iter().take(2).collect::<String>().to_lowercase();
            let is_ord = ["st", "nd", "rd", "th"].contains(&suffix.as_str()) && !chars.get(i + 2).is_some_and(|x| x.is_alphanumeric());
            if is_ord && !plain {
                out += &format!(" {} ", ordinal(num.parse().unwrap_or(0)));
                i += 2;
            } else {
                out += &format!(" {} ", number_words(&num, plain));
            }
            continue;
        }
        match c {
            '%' => out += " percent ",
            '€' => out += " euros ",
            '&' => out += " and ",
            '+' => out += " plus ",
            '$' => out += " dollars ",
            '#' | '_' | '*' | '"' | ':' | ';' => out.push(' '),
            c if c.is_ascii() => out.push(c),
            // accented letters lose their accent; others are dropped
            c => {
                if let Some(base) = deaccent(c) {
                    out.push(base);
                }
            }
        }
        i += 1;
    }
    out.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn deaccent(c: char) -> Option<char> {
    match c {
        '’' | '‘' => Some('\''),
        c => crate::lipsync_text::base_letter(c),
    }
}
