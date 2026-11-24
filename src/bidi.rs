//! Right-to-left text, put in the order it is read in: a title in Hebrew or
//! Arabic, alone or with Latin words and numbers in it.
//!
//! egui lays a line out left to right, run after run - a run being the
//! letters one font writes; within a run of Hebrew or Arabic letters the
//! shaper already sets them right to left (and joins Arabic's). What it
//! does not do is order the runs: "שלום עולם" came out as its two words,
//! each read rightly, in the wrong order, and a line that begins in Hebrew
//! ran from the left. [`seen`] orders a line's parts as the Unicode
//! bidirectional algorithm does (UAX #9, without its explicit embeddings:
//! the direction of the line from its first strong letter, numbers left to
//! right, a pair of brackets of one direction, what is neutral by its
//! neighbours, brackets mirrored) and leaves
//! each run of right-to-left letters as it is written, for the shaper.
//!
//! For what is shown only: the text kept, sorted, searched and saved stays
//! as it was written.

use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// Left to right: Latin, Greek, Cyrillic, CJK ...
    L,
    /// Right to left: Hebrew, Arabic, Syriac, Thaana, N'Ko.
    R,
    /// A European digit; an Arabic-Indic one.
    En,
    An,
    /// A mark on the letter before it.
    Mark,
    /// A sign between or beside digits (. , : / + - % $ #).
    NumberSign,
    /// Space, punctuation, symbols.
    Neutral,
}

/// A letter of a script written right to left (its marks and its own
/// punctuation too: what a font of that script writes).
fn in_rtl_script(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08ff | 0xfb1d..=0xfdff | 0xfe70..=0xfeff | 0x10800..=0x10fff | 0x1e800..=0x1efff)
}

fn class(c: char) -> Class {
    let u = c as u32;
    match u {
        0x30..=0x39 | 0x06f0..=0x06f9 => Class::En,
        0x0660..=0x0669 | 0x066b | 0x066c => Class::An,
        // Hebrew points, Arabic vowel signs, combining marks
        0x0300..=0x036f
        | 0x0591..=0x05bd
        | 0x05bf
        | 0x05c1
        | 0x05c2
        | 0x05c4
        | 0x05c5
        | 0x05c7
        | 0x0610..=0x061a
        | 0x064b..=0x065f
        | 0x0670
        | 0x06d6..=0x06dc
        | 0x06df..=0x06e4
        | 0x06e7
        | 0x06e8
        | 0x06ea..=0x06ed
        | 0x200c
        | 0x200d
        | 0xfe00..=0xfe0f => Class::Mark,
        0x2b | 0x2d | 0x2e | 0x2c | 0x3a | 0x2f | 0x23 | 0x24 | 0x25 | 0xa0 | 0x060c => {
            Class::NumberSign
        }
        _ if in_rtl_script(c) => Class::R,
        _ if c.is_alphabetic() => Class::L,
        _ => Class::Neutral,
    }
}

/// The other of a pair of brackets.
fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        other => other,
    }
}

/// The characters of `text` with the level each is set at (even: left to
/// right, odd: right to left), or None where nothing in it is written right
/// to left.
fn levels(text: &str) -> Option<(Vec<char>, Vec<u8>)> {
    if !text.chars().any(in_rtl_script) {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut classes: Vec<Class> = chars.iter().map(|c| class(*c)).collect();
    let n = chars.len();
    // the line's direction: its first strong letter's
    let base: u8 = match classes.iter().find(|c| matches!(c, Class::L | Class::R)) {
        Some(Class::R) => 1,
        _ => 0,
    };
    let strong_of_base = if base == 1 { Class::R } else { Class::L };
    // a mark is of its letter
    for i in 0..n {
        if classes[i] == Class::Mark {
            classes[i] = if i > 0 {
                classes[i - 1]
            } else {
                Class::Neutral
            };
        }
    }
    // a sign between two digits, or beside one (- % $ #), is of the number
    for i in 0..n {
        if classes[i] != Class::NumberSign {
            continue;
        }
        let digit = |j: Option<usize>| {
            j.and_then(|j| classes.get(j))
                .filter(|c| matches!(c, Class::En | Class::An))
                .copied()
        };
        let (before, after) = (digit(i.checked_sub(1)), digit(Some(i + 1)));
        let ends = matches!(chars[i], '%' | '$' | '#' | '+' | '-');
        classes[i] = match (before, after) {
            (Some(a), Some(b)) if a == b => a,
            (Some(a), None) | (None, Some(a)) if ends => a,
            _ => Class::Neutral,
        };
    }
    // a European number after a left-to-right letter is of the letter's run
    let mut last_strong = strong_of_base;
    for class in classes.iter_mut() {
        match *class {
            Class::L | Class::R => last_strong = *class,
            Class::En if last_strong == Class::L => *class = Class::L,
            _ => {}
        }
    }
    // what is neutral: between two of a direction, that direction (a number
    // counts as right to left there); else the line's
    let side = |c: Class| match c {
        Class::L => Some(Class::L),
        Class::R | Class::En | Class::An => Some(Class::R),
        _ => None,
    };
    // a pair of brackets is of one direction (UAX #9, N0): the line's
    // where a letter of that direction stands in it; where only letters
    // of the other do, the other's if such a letter stands before the
    // pair too, else the line's. ("Habaytak (حبيتك) 1970" was shown as
    // "Habaytak (1970 (حبيتك": the closing bracket, between an Arabic
    // word and a number, went with them - review 20, L4.)
    let closing = |c: char| match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    };
    let mut open: Vec<(usize, char)> = Vec::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        if classes[i] != Class::Neutral {
            continue;
        }
        if let Some(close) = closing(chars[i]) {
            if open.len() >= 63 {
                break;
            }
            open.push((i, close));
        } else if let Some(k) = open.iter().rposition(|(_, close)| *close == chars[i]) {
            pairs.push((open[k].0, i));
            open.truncate(k);
        }
    }
    pairs.sort_unstable();
    let other = if base == 1 { Class::L } else { Class::R };
    for (from, to) in pairs {
        let within = |want: Class| (from + 1..to).any(|j| side(classes[j]) == Some(want));
        let taken = if within(strong_of_base) {
            strong_of_base
        } else if within(other) {
            let before = (0..from)
                .rev()
                .find_map(|j| side(classes[j]))
                .unwrap_or(strong_of_base);
            if before == other {
                other
            } else {
                strong_of_base
            }
        } else {
            continue;
        };
        classes[from] = taken;
        classes[to] = taken;
    }
    let mut i = 0;
    while i < n {
        if classes[i] != Class::Neutral {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && classes[i] == Class::Neutral {
            i += 1;
        }
        let before = (0..start)
            .rev()
            .find_map(|j| side(classes[j]))
            .unwrap_or(strong_of_base);
        let after = (i..n)
            .find_map(|j| side(classes[j]))
            .unwrap_or(strong_of_base);
        let taken = if before == after {
            before
        } else {
            strong_of_base
        };
        for class in &mut classes[start..i] {
            *class = taken;
        }
    }
    let mut levels: Vec<u8> = classes
        .iter()
        .map(|c| match (base, c) {
            (0, Class::L) => 0,
            (0, Class::R) => 1,
            (0, _) => 2,
            (_, Class::R) => 1,
            _ => 2,
        })
        .collect();
    // spaces at the line's end: the line's
    for i in (0..n).rev() {
        if !chars[i].is_whitespace() {
            break;
        }
        levels[i] = base;
    }
    Some((chars, levels))
}

/// The places of `levels`' characters in the order they are seen in, left
/// to right (UAX #9, L2: from the highest level down, every stretch at that
/// level or higher turned round).
fn order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let highest = levels.iter().copied().max().unwrap_or(0);
    for level in (1..=highest).rev() {
        let mut i = 0;
        while i < order.len() {
            if levels[order[i]] < level {
                i += 1;
                continue;
            }
            let start = i;
            while i < order.len() && levels[order[i]] >= level {
                i += 1;
            }
            order[start..i].reverse();
        }
    }
    order
}

/// `text` as it is seen, character by character from the left (what a
/// renderer that sets every character left to right would be given).
pub fn visual(text: &str) -> Cow<'_, str> {
    let Some((chars, levels)) = levels(text) else {
        return Cow::Borrowed(text);
    };
    Cow::Owned(
        order(&levels)
            .into_iter()
            .map(|i| {
                if levels[i] % 2 == 1 {
                    mirrored(chars[i])
                } else {
                    chars[i]
                }
            })
            .collect(),
    )
}

/// `text` for egui: its parts in the order they are seen in, each run of
/// right-to-left letters as it is written (the shaper turns it round and
/// joins its letters). Borrowed where nothing in it is written right to
/// left.
pub fn seen(text: &str) -> Cow<'_, str> {
    let Some((chars, levels)) = levels(text) else {
        return Cow::Borrowed(text);
    };
    let mut order = order(&levels);
    // (a run of the right-to-left script's own characters, turned round
    // above: back as written)
    let letter = |i: usize| levels[i] % 2 == 1 && in_rtl_script(chars[i]);
    let mut i = 0;
    while i < order.len() {
        if !letter(order[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < order.len() && letter(order[i]) {
            i += 1;
        }
        order[start..i].reverse();
    }
    Cow::Owned(
        order
            .into_iter()
            .map(|i| {
                if levels[i] % 2 == 1 {
                    mirrored(chars[i])
                } else {
                    chars[i]
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_right_to_left_letters_is_left_alone() {
        for text in [
            "",
            "Afterglow – Demo Band",
            "日本語の音楽 12",
            "a (b) [c] 1,5 %",
        ] {
            assert!(matches!(seen(text), Cow::Borrowed(_)), "{text:?}");
            assert_eq!(visual(text), text);
        }
    }

    #[test]
    fn right_to_left_text_is_seen_from_the_right() {
        for (written, shown) in [
            // a word, two words: the first at the right
            ("אבג", "גבא"),
            ("שלום עולם", "םלוע םולש"),
            ("بتث", "ثتب"),
            // a Hebrew word in an English line
            ("ab אבג cd", "ab גבא cd"),
            // a line that begins in Hebrew runs from the right; its Latin
            // words and its numbers read left to right in it
            ("אב cd גד", "דג cd בא"),
            ("אבג 12", "12 גבא"),
            ("אב 12-34 גד", "דג 12-34 בא"),
            ("אב 3.5% גד", "דג 3.5% בא"),
            // brackets turn with it
            ("אב (גד)", "(דג) בא"),
            ("אב (cd)", "(cd) בא"),
            // an Arabic title with a year and a Latin name
            ("أغنية 2020 remix", "remix 2020 ةينغأ"),
            // spaces at the end stay at the line's end
            ("אב ", " בא"),
            // a pair of brackets round an Arabic word in an English line,
            // a number after it: the brackets the line's, the year after
            ("Habaytak (حبيتك) 1970", "Habaytak (كتيبح) 1970"),
            (
                "Fairuz - فيروز - Habaytak (حبيتك) 1970 [Live]",
                "Fairuz - زوريف - Habaytak (كتيبح) 1970 [Live]",
            ),
            // (a number after a Hebrew word goes with what follows it)
            ("a [אב] 12 {גד}", "a [בא] {דג} 12"),
            ("a [אב] b 12 {גד}", "a [בא] b 12 {דג}"),
            // (after a right-to-left word the pair is of its run)
            ("ab אב (גד) cd", "ab (דג) בא cd"),
            // (a bracket without its other: as any sign)
            ("אב (גד", "דג) בא"),
        ] {
            assert_eq!(visual(written), shown, "{written:?}");
        }
    }

    #[test]
    fn a_run_of_right_to_left_letters_is_left_for_the_shaper() {
        for (written, given) in [
            ("אבג", "אבג"),
            ("שלום עולם", "עולם שלום"),
            ("ab אבג cd", "ab אבג cd"),
            ("אב cd גד", "גד cd אב"),
            ("אבג 12", "12 אבג"),
            ("אב (cd)", "(cd) אב"),
            ("أغنية 2020 remix", "remix 2020 أغنية"),
        ] {
            assert_eq!(seen(written), given, "{written:?}");
        }
    }
}
