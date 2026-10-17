//! Text helpers shared by the lip-sync languages' preprocessing (TalkingHead's
//! regular expressions, written out).

/// Runs of three or more of the same letter (not digits) shortened to two, as the
/// modules' `(\D)\1\1+` does.
pub(crate) fn squeeze_repeats(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let mut j = i + 1;
        while j < c.len() && c[j] == c[i] {
            j += 1;
        }
        let n = if !c[i].is_ascii_digit() && j - i >= 3 { 2 } else { j - i };
        out.extend(std::iter::repeat_n(c[i], n));
        i = j;
    }
    out
}

/// `(\d)<sep>(\d)` → `$1 <word> $2`, left to right without overlaps, as a global
/// regular expression replaces.
pub(crate) fn decimal_word(s: &str, seps: &[char], word: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if i + 2 < c.len() && c[i].is_ascii_digit() && seps.contains(&c[i + 1]) && c[i + 2].is_ascii_digit() {
            out.push(c[i]);
            out += &format!(" {word} ");
            out.push(c[i + 2]);
            i += 3;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// Every run of digits replaced by `f` of it.
pub(crate) fn numbers_to_words(s: &str, f: &dyn Fn(&str) -> String) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i].is_ascii_digit() {
            let j = (i..c.len()).find(|j| !c[*j].is_ascii_digit()).unwrap_or(c.len());
            out += &f(&c[i..j].iter().collect::<String>());
            i = j;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

/// Whitespace runs as one space.
pub(crate) fn one_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A Latin letter without its accent ("é" → "e"), as `normalize('NFD')[0]` gives; `None`
/// for letters without one.
pub(crate) fn base_letter(c: char) -> Option<char> {
    const MAP: &[(&str, char)] = &[
        ("àáâãäåāăą", 'a'),
        ("çćĉċč", 'c'),
        ("ďđ", 'd'),
        ("èéêëēĕėęě", 'e'),
        ("ĝğġģ", 'g'),
        ("ĥħ", 'h'),
        ("ìíîïĩīĭįı", 'i'),
        ("ĵ", 'j'),
        ("ķ", 'k'),
        ("ĺļľŀł", 'l'),
        ("ñńņňŉ", 'n'),
        ("òóôõöōŏőø", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşš", 's'),
        ("ţťŧ", 't'),
        ("ùúûüũūŭůűų", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    let lower = c.to_lowercase().next().unwrap_or(c);
    let base = MAP.iter().find(|(set, _)| set.contains(lower)).map(|(_, b)| *b)?;
    Some(if c.is_uppercase() { base.to_ascii_uppercase() } else { base })
}
