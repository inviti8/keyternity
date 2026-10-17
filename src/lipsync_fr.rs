//! French text to sounds for lip sync (docs/LIP_SYNC.md §7.3).
//!
//! Ported from TalkingHead's `lipsync-fr.mjs` (https://github.com/met4citizen/TalkingHead),
//! by Stephan Wald, MIT License (full notice in NOTICE). The rule table is the
//! original's, verbatim; `L` is sounded where its rules sound an L (§6.1). Fixed from
//! the original: a rule applies only where it matches at the current letter (the
//! original's regular expressions aren't anchored, so "bonjour" lost its "o" to the
//! "ou" further on); `faithful_sounds` keeps the original's reading, for the tests.

use crate::lipsync_rules::{Classes, RuleLang, Sound};
use crate::lipsync_text::*;

const RULES: &[(&str, &[&str])] = &[
    ("A", &[
        "[AN]C=aa nn", "[AN]G=aa nn", "[AN]T=aa nn", "[AN]D=aa nn", "[AN] =aa nn",
        "[AN]$=aa nn", "[AM]P=aa nn", "[AM]B=aa nn", "[AM] =aa nn", "[AM]$=aa nn", "[AI]N=E nn",
        "[AIM]=E nn", "[AIN]=E nn", "[AU]=O", "[AUX]=O", "[AUT]=O", "[AI]=E", "[AY]=E",
        "[A]=aa",
    ]),
    ("À", &[
        "[À]=aa",
    ]),
    ("Â", &[
        "[Â]=aa",
    ]),
    ("B", &[
        " [B] =PP", "[BB]=PP", "[B]=PP",
    ]),
    ("C", &[
        "[C]E=SS", "[C]I=SS", "[C]Y=SS", "[C]È=SS", "[C]É=SS", "[C]Ê=SS", "[CH]=SS", "[C]A=kk",
        "[C]O=kk", "[C]U=kk", "[C]L=kk", "[C]R=kk", "[CK]=kk", "[C]=kk",
    ]),
    ("Ç", &[
        "[Ç]=SS",
    ]),
    ("D", &[
        "[D]=DD",
    ]),
    ("E", &[
        "[EN]C=aa nn", "[EN]T=aa nn", "[EN]D=aa nn", "[EN] =aa nn", "[EN]$=aa nn",
        "[EM]P=aa nn", "[EM]B=aa nn", "[EM] =aa nn", "[EM]$=aa nn", "[EAU]=O", "[EAU]X=O",
        "[EU]=U", "[EUX]=U", "[EUR]=U RR", "[EI]=E", "[EIN]=E nn", "[ER] =E", "[ER]$=E",
        "[EZ] =E", "[EZ]$=E", "[ED] =E", "[ED]$=E", " [E] =", "[E] =", "[E]$=", "[E]S =",
        "[E]S$=", "[E]NT =", "[E]NT$=", "[È]=E", "[É]=E", "[Ê]=E", "[Ë]=E", "[E]=E",
    ]),
    ("È", &[
        "[È]=E",
    ]),
    ("É", &[
        "[É]=E",
    ]),
    ("Ê", &[
        "[Ê]=E",
    ]),
    ("Ë", &[
        "[Ë]=E",
    ]),
    ("F", &[
        "[FF]=FF", "[F]=FF", "[PH]=FF",
    ]),
    ("G", &[
        "[G]E=SS", "[G]I=SS", "[G]Y=SS", "[G]È=SS", "[G]É=SS", "[G]Ê=SS", "[GN]=nn I",
        "[GN]E=nn", "[GN]A=nn aa", "[GN]O=nn O", "[GU]E=kk", "[GU]I=kk", "[GU]A=kk FF aa",
        "[GU]O=kk FF O", "[G]A=kk", "[G]O=kk", "[G]U=kk", "[G]L=kk", "[G]R=kk", "[GG]=kk",
        "[G]=kk",
    ]),
    ("H", &[
        "[H]=",
    ]),
    ("I", &[
        "[IN]C=E nn", "[IN]T=E nn", "[IN]D=E nn", "[IN] =E nn", "[IN]$=E nn", "[IM]P=E nn",
        "[IM]B=E nn", "[IM] =E nn", "[IM]$=E nn", "[IEN]=I E nn", "[IER]=I E", "[IEU]=I U",
        "[IEZ]=I E", "[ILL]E=I", "[ILLE]=I", "[ILL]=I", "[Î]=I", "[Ï]=I", "[I]=I",
    ]),
    ("Î", &[
        "[Î]=I",
    ]),
    ("Ï", &[
        "[Ï]=I",
    ]),
    ("J", &[
        "[J]=SS",
    ]),
    ("K", &[
        "[K]=kk",
    ]),
    ("L", &[
        "[LL]E=", "[LLE]=", "[LL]A=I aa", "[LL]O=I O", "[LL]U=I U", "[LL]I=I", "[LL]=I",
        "[L]=nn",
    ]),
    ("M", &[
        "[MM]=PP", "[M]=PP",
    ]),
    ("N", &[
        "[NN]=nn", "[N]=nn",
    ]),
    ("O", &[
        "[ON]C=O nn", "[ON]T=O nn", "[ON]D=O nn", "[ON] =O nn", "[ON]$=O nn", "[OM]P=O nn",
        "[OM]B=O nn", "[OM] =O nn", "[OM]$=O nn", "[OI]N=FF E nn", "[OI]G=FF aa", "[OI]S=FF aa",
        "[OI]T=FF aa", "[OI]X=FF aa", "[OI]=FF aa", "[OU]=U", "[OÙ]=U", "[OÛ]=U", "[OEU]=U",
        "[OEUR]=U RR", "[OUGH]=U FF", "[OUGH]T=U", "[Ô]=O", "[O]=O",
    ]),
    ("Ô", &[
        "[Ô]=O",
    ]),
    ("Ù", &[
        "[Ù]=U",
    ]),
    ("Û", &[
        "[Û]=U",
    ]),
    ("P", &[
        "[PH]=FF", "[PP]=PP", "[P]=PP",
    ]),
    ("Q", &[
        "[QU]=kk", "[Q]=kk",
    ]),
    ("R", &[
        "[RR]=RR", "[R]=RR",
    ]),
    ("S", &[
        "#[S]#=SS", " [S]=SS", "[SS]=SS", "[S] =", "[S]$=", "[SC]E=SS", "[SC]I=SS", "[SC]Y=SS",
        "[S]=SS",
    ]),
    ("T", &[
        "[TI]A=SS I aa", "[TI]E=SS I E", "[TI]O=SS I O", "[TI]ON=SS I O nn", "[TH]=DD",
        "[TT]=DD", "[T] =", "[T]$=", "[T]=DD",
    ]),
    ("U", &[
        "[UN]C=U nn", "[UN]T=U nn", "[UN]D=U nn", "[UN] =U nn", "[UN]$=U nn", "[UM]=U nn",
        "[UE]=I", "[UEI]=I E", "[UEIL]=I I", "[UILL]=I", "[Ù]=U", "[Û]=U", "[Ü]=I U", "[U]=I U",
    ]),
    ("Ü", &[
        "[Ü]=I U",
    ]),
    ("V", &[
        "[V]=FF",
    ]),
    ("W", &[
        "[W]=FF",
    ]),
    ("X", &[
        "[X] =", "[X]$=", "#[X]#=kk SS", " [X]=kk SS", "[X]=kk SS",
    ]),
    ("Y", &[
        "[Y]=I", " [Y]=I", "[YE]=I E", "[YA]=I aa", "[YO]=I O", "[YU]=I U",
    ]),
    ("Z", &[
        "[Z]=SS",
    ]),
];

fn duration(sound: &str) -> f32 {
    match sound {
        "aa" | "TH" | "FF" => 1.,
        "E" | "U" => 0.95,
        "I" => 0.9,
        "O" | "DD" => 1.05,
        "PP" => 1.1,
        "SS" => 1.25,
        "kk" => 1.2,
        "nn" | "L" => 0.88,
        "RR" => 1.15,
        _ => 1.,
    }
}

fn pause(c: char) -> f32 {
    match c {
        ' ' => 1.,
        ',' => 2.5,
        '.' => 3.5,
        ';' => 2.8,
        ':' => 2.2,
        '!' | '?' => 3.2,
        '-' => 0.8,
        '\'' | '"' => 0.3,
        '(' | ')' => 1.5,
        _ => 0.,
    }
}

fn classes() -> Classes {
    Classes {
        vowels: "AEIOUYÀÂÈÉÊËÎÏÔÙÛÜ",
        voiced: "BDVGJLMNRWZ",
        suffixes: &["ER", "E", "ES", "ÉS", "ÈS", "ÊS", "ENT", "MENT", "TION", "SION"],
        sibilant: ("SCGZXJ", &["CH", "SH", "GN"]),
        alveolar: ("TSRDLZNJ", &["TH", "CH", "SH", "GN"]),
        consonants: "BCDFGHJKLMNPQRSTVWXZÇ",
        consonant_run: true,
        front: "EIYÈÉÊËÎÏ",
    }
}

/// A word's sounds, each rule matched at the current letter.
pub fn word_sounds(word: &str) -> Vec<Sound> {
    static L: std::sync::OnceLock<RuleLang> = std::sync::OnceLock::new();
    L.get_or_init(|| RuleLang::new(RULES, classes(), false, true, duration, pause)).sounds(word)
}

/// A word's sounds as the original module reads them: its rules match anywhere in the
/// word (its regular expressions aren't anchored), so "bonjour" loses its "o" to the
/// "ou" further on. Only for checking the port against the original.
pub fn faithful_sounds(word: &str) -> Vec<Sound> {
    static L: std::sync::OnceLock<RuleLang> = std::sync::OnceLock::new();
    L.get_or_init(|| RuleLang::new(RULES, classes(), true, true, duration, pause)).sounds(word)
}

const DIGITS: [&str; 10] = ["zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf"];
const ONES: [&str; 10] = ["", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf"];
const TEENS: [&str; 10] = ["dix", "onze", "douze", "treize", "quatorze", "quinze", "seize", "dix-sept", "dix-huit", "dix-neuf"];
const TENS: [&str; 10] = ["", "dix", "vingt", "trente", "quarante", "cinquante", "soixante", "soixante-dix", "quatre-vingts", "quatre-vingt-dix"];

fn tens(n: u64) -> String {
    if n < 10 {
        return ONES[n as usize].to_string();
    }
    if n < 20 {
        return TEENS[(n - 10) as usize].to_string();
    }
    if (70..80).contains(&n) {
        return if n == 71 { "soixante et onze".into() } else { format!("soixante-{}", TEENS[(n - 70) as usize]) };
    }
    if n >= 90 {
        return if n == 91 { "quatre-vingt-onze".into() } else { format!("quatre-vingt-{}", TEENS[(n - 90) as usize]) };
    }
    let (t, o) = (n / 10, n % 10);
    if t == 8 && o == 0 {
        "quatre-vingts".into()
    } else if t == 8 {
        format!("quatre-vingt-{}", ONES[o as usize])
    } else if (2..=6).contains(&t) && o == 1 {
        format!("{} et un", TENS[t as usize])
    } else if o == 0 {
        TENS[t as usize].to_string()
    } else {
        format!("{}-{}", TENS[t as usize], ONES[o as usize])
    }
}

fn hundreds(n: u64) -> String {
    if n < 100 {
        return tens(n);
    }
    let (h, r) = (n / 100, n % 100);
    let mut s = if h == 1 { "cent".to_string() } else { format!("{} cent{}", ONES[h as usize], if r == 0 { "s" } else { "" }) };
    if r > 0 {
        s += &format!(" {}", tens(r));
    }
    s
}

fn thousands(n: u64) -> String {
    if n < 1000 {
        return hundreds(n);
    }
    let mut s = if n / 1000 == 1 { "mille".to_string() } else { format!("{} mille", hundreds(n / 1000)) };
    if n % 1000 > 0 {
        s += &format!(" {}", hundreds(n % 1000));
    }
    s
}

fn millions(n: u64) -> String {
    if n < 1_000_000 {
        return thousands(n);
    }
    let m = n / 1_000_000;
    let mut s = if m == 1 { "un million".to_string() } else { format!("{} millions", hundreds(m)) };
    if n % 1_000_000 > 0 {
        s += &format!(" {}", thousands(n % 1_000_000));
    }
    s
}

/// A number read aloud (codes with a leading zero digit by digit).
pub fn number_words(s: &str) -> String {
    if s == "0" {
        return "zéro".into();
    }
    let digits = || s.chars().filter_map(|d| d.to_digit(10)).map(|d| DIGITS[d as usize]).collect::<Vec<_>>().join(" ");
    match s.parse::<u64>() {
        Ok(n) if !s.starts_with('0') => millions(n),
        _ => digits(),
    }
}

/// Text ready for the rules (the module's `preProcessText`).
pub fn preprocess(text: &str) -> String {
    let s: String = text.chars().filter(|c| !"#_*\":;".contains(*c)).collect();
    let mut t = String::new();
    for c in s.chars() {
        match c {
            '%' => t += " pourcent ",
            '€' => t += " euros ",
            '&' => t += " et ",
            '+' => t += " plus ",
            '$' => t += " dollars ",
            '=' => t += " égale ",
            '@' => t += " arobase ",
            '°' => t += " degrés ",
            c => t.push(c),
        }
    }
    let t = decimal_word(&t, &[',', '.'], "virgule");
    let t = numbers_to_words(&t, &number_words);
    one_space(&squeeze_repeats(&t)).trim().to_string()
}
