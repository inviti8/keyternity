//! The letter-to-sound rule engine behind the rule-based languages (docs/LIP_SYNC.md
//! §7.3): TalkingHead's `left[letters]right=sounds` rules, matched as its modules do,
//! without regular expressions.
//!
//! A rule's contexts use symbols each language defines (`Classes`): `#` one or more
//! vowels, `.` a voiced consonant, `%` a suffix, `&` and `@` consonant groups, `^` a
//! consonant (French: one or more), `+` a front vowel, `:` zero or more consonants, a
//! space for the word's edge (JavaScript's ASCII `\b`), and `$` the word's end.

/// A sound in a word: (sound, start, length), in relative units (1 ≈ an average sound).
pub type Sound = (&'static str, f32, f32);

/// What a language's context symbols stand for.
pub struct Classes {
    /// `#`
    pub vowels: &'static str,
    /// `.`
    pub voiced: &'static str,
    /// `%`
    pub suffixes: &'static [&'static str],
    /// `&`: one letter of the first, or one of the groups
    pub sibilant: (&'static str, &'static [&'static str]),
    /// `@`
    pub alveolar: (&'static str, &'static [&'static str]),
    /// `^` (one, or one or more with `consonant_run`) and `:` (zero or more)
    pub consonants: &'static str,
    pub consonant_run: bool,
    /// `+`
    pub front: &'static str,
}

/// A rule-based language: its rules (letter → rules, tried in order), classes, and how
/// rules match.
pub struct RuleLang {
    rules: Vec<(char, Vec<Rule>)>,
    classes: Classes,
    /// French's rules match case-insensitively anywhere in the word, not only at the
    /// current letter (its module's regular expressions aren't anchored); kept so the
    /// port reads as the original does
    anywhere: bool,
    duration: fn(&str) -> f32,
    pause: fn(char) -> f32,
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Lit(char),
    Vowels,
    Voiced,
    Suffix,
    Sibilant,
    Alveolar,
    Consonant,
    FrontVowel,
    Consonants,
    Edge,
    End,
}

struct Rule {
    left: Vec<Tok>,
    letters: Vec<char>,
    right: Vec<Tok>,
    sounds: Vec<&'static str>,
}

fn toks(s: &str) -> Vec<Tok> {
    s.chars()
        .map(|c| match c {
            '#' => Tok::Vowels,
            '.' => Tok::Voiced,
            '%' => Tok::Suffix,
            '&' => Tok::Sibilant,
            '@' => Tok::Alveolar,
            '^' => Tok::Consonant,
            '+' => Tok::FrontVowel,
            ':' => Tok::Consonants,
            ' ' => Tok::Edge,
            '$' => Tok::End,
            c => Tok::Lit(c),
        })
        .collect()
}

/// `l_sound`: where a rule's letters sound an L and no N, its `nn` is the L sound
/// (TalkingHead folds L into `nn`; Preston Blair draws it apart, §6.1).
fn parse(rule: &'static str, l_sound: bool) -> Rule {
    let l = rule.find('[').unwrap();
    let r = rule.find(']').unwrap();
    let e = rule.find('=').unwrap();
    let letters: Vec<char> = rule[l + 1..r].chars().collect();
    let mut sounds: Vec<&'static str> = rule[e + 1..].split(' ').filter(|s| !s.is_empty()).collect();
    if l_sound && letters.contains(&'L') && !letters.contains(&'N') {
        if let Some(i) = sounds.iter().rposition(|s| *s == "nn") {
            sounds[i] = "L";
        }
    }
    Rule { left: toks(&rule[..l]), letters, right: toks(&rule[r + 1..e]), sounds }
}

impl RuleLang {
    pub fn new(
        table: &'static [(&'static str, &'static [&'static str])],
        classes: Classes,
        anywhere: bool,
        l_sound: bool,
        duration: fn(&str) -> f32,
        pause: fn(char) -> f32,
    ) -> RuleLang {
        let rules = table.iter().map(|(k, rs)| (k.chars().next().unwrap(), rs.iter().map(|r| parse(r, l_sound)).collect())).collect();
        RuleLang { rules, classes, anywhere, duration, pause }
    }

    fn is_vowel(&self, c: char) -> bool {
        self.classes.vowels.contains(c)
    }
    fn is_consonant(&self, c: char) -> bool {
        self.classes.consonants.contains(c)
    }

    /// The positions one token can take `p` to (forward: after it; backward: before).
    fn step(&self, t: Tok, s: &[char], p: usize, fwd: bool) -> Vec<usize> {
        let one = |ok: &dyn Fn(char) -> bool| -> Vec<usize> {
            if fwd {
                s.get(p).filter(|c| ok(**c)).map(|_| vec![p + 1]).unwrap_or_default()
            } else if p > 0 && ok(s[p - 1]) {
                vec![p - 1]
            } else {
                vec![]
            }
        };
        let run = |ok: &dyn Fn(char) -> bool, min: usize| -> Vec<usize> {
            let mut out = vec![];
            let mut n = 0;
            loop {
                if n >= min {
                    out.push(if fwd { p + n } else { p - n });
                }
                let next = if fwd { s.get(p + n).copied() } else { p.checked_sub(n + 1).map(|i| s[i]) };
                match next {
                    Some(c) if ok(c) => n += 1,
                    _ => break,
                }
            }
            out
        };
        let alts = |alts: &[&str]| -> Vec<usize> {
            alts.iter()
                .filter_map(|a| {
                    let a: Vec<char> = a.chars().collect();
                    if fwd {
                        (s.len() >= p + a.len() && s[p..p + a.len()] == a[..]).then_some(p + a.len())
                    } else {
                        (p >= a.len() && s[p - a.len()..p] == a[..]).then(|| p - a.len())
                    }
                })
                .collect()
        };
        let c = &self.classes;
        match t {
            Tok::Lit(l) => one(&|x| x == l),
            Tok::Vowels => run(&|x| self.is_vowel(x), 1),
            Tok::Consonants => run(&|x| self.is_consonant(x), 0),
            Tok::Consonant if c.consonant_run => run(&|x| self.is_consonant(x), 1),
            Tok::Consonant => one(&|x| self.is_consonant(x)),
            Tok::Voiced => one(&|x| c.voiced.contains(x)),
            Tok::FrontVowel => one(&|x| c.front.contains(x)),
            Tok::Suffix => alts(c.suffixes),
            Tok::Sibilant => {
                let mut v = one(&|x| c.sibilant.0.contains(x));
                v.extend(alts(c.sibilant.1));
                v
            }
            Tok::Alveolar => {
                let mut v = one(&|x| c.alveolar.0.contains(x));
                v.extend(alts(c.alveolar.1));
                v
            }
            Tok::Edge => {
                // JavaScript's `\b`: word characters are ASCII letters, digits and `_`
                let w = |c: char| c.is_ascii_alphanumeric() || c == '_';
                let before = p > 0 && w(s[p - 1]);
                let after = p < s.len() && w(s[p]);
                if before != after { vec![p] } else { vec![] }
            }
            Tok::End => {
                if p == s.len() { vec![p] } else { vec![] }
            }
        }
    }

    fn matches(&self, ts: &[Tok], s: &[char], p: usize, fwd: bool) -> bool {
        let Some((t, rest)) = (if fwd { ts.split_first() } else { ts.split_last() }) else { return true };
        self.step(*t, s, p, fwd).into_iter().any(|q| self.matches(rest, s, q, fwd))
    }

    fn rule_at(&self, r: &Rule, w: &[char], i: usize) -> bool {
        w[i..].starts_with(&r.letters) && self.matches(&r.left, w, i, false) && self.matches(&r.right, w, i + r.letters.len(), true)
    }

    /// A word's sounds, with relative starts and lengths (TalkingHead's
    /// `wordsToVisemes`). A sound repeated straight after itself lengthens the first.
    pub fn sounds(&self, word: &str) -> Vec<Sound> {
        let w: Vec<char> = word.to_uppercase().chars().collect();
        let mut out: Vec<Sound> = vec![];
        let mut t = 0.;
        let mut i = 0;
        while i < w.len() {
            let found = self.rules.iter().find(|(c, _)| *c == w[i]).and_then(|(_, set)| {
                set.iter().find(|r| {
                    if self.anywhere {
                        // the rule's first letter may match the current one in either case;
                        // everything else is compared as written (upper case)
                        (0..w.len()).any(|p| self.rule_at(r, &w, p))
                    } else {
                        self.rule_at(r, &w, i)
                    }
                })
            });
            let Some(rule) = found else {
                t += (self.pause)(w[i]);
                i += 1;
                continue;
            };
            for s in &rule.sounds {
                match out.last_mut() {
                    Some(last) if last.0 == *s => {
                        let d = 0.7 * (self.duration)(s);
                        last.2 += d;
                        t += d;
                    }
                    _ => {
                        let d = (self.duration)(s);
                        out.push((s, t, d));
                        t += d;
                    }
                }
            }
            i += rule.letters.len();
        }
        out
    }
}

/// Sounds from a letter table (Finnish, Lithuanian): each letter its sound, a repeat
/// lengthening the one before. `sound` gives a letter's sound and length.
pub fn letter_sounds(word: &str, sound: fn(char) -> Option<(&'static str, f32)>, pause: fn(char) -> f32) -> Vec<Sound> {
    let mut out: Vec<Sound> = vec![];
    let mut t = 0.;
    for c in word.chars().flat_map(char::to_lowercase) {
        match sound(c) {
            Some((s, d)) => match out.last_mut() {
                Some(last) if last.0 == s => {
                    last.2 += 0.7 * d;
                    t += 0.7 * d;
                }
                _ => {
                    out.push((s, t, d));
                    t += d;
                }
            },
            None => t += pause(c),
        }
    }
    out
}
