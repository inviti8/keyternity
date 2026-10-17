//! Lip sync's other languages (docs/LIP_SYNC.md §7.3) against the originals.
//!
//! `tests/data/lipsync_<lang>_reference.tsv` is the output of TalkingHead's own modules
//! (MIT; English, Finnish and Lithuanian by Mika Suominen, German and French by Stephan
//! Wald, Brazilian Portuguese by Lucas Boemeke) on short texts: `P`, a line and its
//! preprocessed text; `W`, a word, its visemes, starts and lengths.

use keyternity_lib::lipsync::{self, Language};

/// The port sounds L apart; the originals fold it into `nn`.
fn folded(sounds: &[(&str, f32, f32)]) -> String {
    let mut v: Vec<&str> = sounds.iter().map(|s| if s.0 == "L" { "nn" } else { s.0 }).collect();
    v.dedup();
    v.join(" ")
}

fn check(lang: Language, reference: &str, normalize: &dyn Fn(&str) -> String) {
    check_with(lang, reference, normalize, &|w| lang.sounds(w));
}

fn check_with(lang: Language, reference: &str, normalize: &dyn Fn(&str) -> String, sounds: &dyn Fn(&str) -> Vec<(&'static str, f32, f32)>) {
    let (mut words, mut lines) = (0, 0);
    for line in reference.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols[0] {
            "P" => {
                let ours: Vec<String> = normalize(&lang.preprocess(cols[1])).split_whitespace().map(String::from).collect();
                let theirs: Vec<String> = cols[2].split_whitespace().map(String::from).collect();
                assert_eq!(ours, theirs, "{lang:?} preprocessing {:?}", cols[1]);
                lines += 1;
            }
            "W" => {
                let ours = sounds(cols[1]);
                assert_eq!(folded(&ours), cols[2], "{lang:?} {:?}", cols[1]);
                // where nothing was folded, the timing matches too
                if !ours.iter().any(|s| s.0 == "L") {
                    let starts: Vec<String> = ours.iter().map(|s| format!("{:.3}", s.1)).collect();
                    let lens: Vec<String> = ours.iter().map(|s| format!("{:.3}", s.2)).collect();
                    assert_eq!(starts.join(" "), cols.get(3).copied().unwrap_or(""), "{lang:?} {:?} starts", cols[1]);
                    assert_eq!(lens.join(" "), cols.get(4).copied().unwrap_or(""), "{lang:?} {:?} lengths", cols[1]);
                }
                words += 1;
            }
            _ => {}
        }
    }
    assert!(words > 25 && lines >= 4, "{lang:?}: {words} words, {lines} lines");
}

#[test]
fn german_matches_talkinghead() {
    check(Language::German, include_str!("../data/lipsync_de_reference.tsv"), &|s| s.to_string());
}

#[test]
fn french_matches_talkinghead() {
    // the rules as ported, read as the original reads them
    let reference = include_str!("../data/lipsync_fr_reference.tsv");
    check_with(Language::French, reference, &|s| s.to_string(), &keyternity_lib::lipsync_fr::faithful_sounds);
}

#[test]
fn french_rules_apply_at_the_current_letter() {
    let s = |w: &str| Language::French.sounds(w).iter().map(|s| s.0).collect::<Vec<_>>().join(" ");
    assert_eq!(s("bonjour"), "PP O nn SS U RR", "the original read PP U SS U RR");
}

#[test]
fn portuguese_matches_talkinghead() {
    check(Language::PortugueseBr, include_str!("../data/lipsync_pt_br_reference.tsv"), &|s| s.to_string());
}

#[test]
fn finnish_matches_talkinghead() {
    check(Language::Finnish, include_str!("../data/lipsync_fi_reference.tsv"), &|s| s.to_string());
}

#[test]
fn lithuanian_matches_talkinghead_but_keeps_its_carons() {
    // the original's accent stripping also removes š, ž and č's carons
    let strip = |s: &str| s.chars().map(|c| match c { 'š' => 's', 'ž' => 'z', 'č' => 'c', 'Š' => 'S', 'Ž' => 'Z', 'Č' => 'C', c => c }).collect();
    check(Language::Lithuanian, include_str!("../data/lipsync_lt_reference.tsv"), &strip);
    let w: Vec<&str> = Language::Lithuanian.sounds("šuo").iter().map(|s| s.0).collect();
    assert_eq!(w, vec!["CH", "U", "O"], "š is its own sound");
}

#[test]
fn fixed_numbers() {
    assert_eq!(Language::Finnish.preprocess("20"), "kaksikymmentä");
    assert_eq!(Language::Lithuanian.preprocess("11"), "vienuolika");
    assert_eq!(Language::Lithuanian.preprocess("21"), "dvidešimt vienas");
}

#[test]
fn every_language_makes_words_with_sounds() {
    for (lang, text) in [
        (Language::English, "Hello there"),
        (Language::German, "Guten Morgen, schöne Grüße"),
        (Language::French, "Bonjour, ça va très bien"),
        (Language::PortugueseBr, "Bom dia, tudo bem"),
        (Language::Finnish, "Hyvää huomenta"),
        (Language::Lithuanian, "Labas rytas"),
    ] {
        let words = lipsync::words_of(text, lang);
        assert!(words.len() >= 2, "{lang:?}");
        assert!(words.iter().all(|(w, _)| !lang.sounds(w).is_empty()), "{lang:?}: {words:?}");
    }
}
