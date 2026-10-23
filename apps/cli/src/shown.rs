//! Names from the server, made safe to print: other people chose them, and a terminal acts on
//! some characters instead of showing them.
//!
//! A control character (C0, DEL, C1) can move the cursor, clear the screen or set the clipboard;
//! a bidi control makes a name read like another (`Ada\u{202e}…`); invisible format characters
//! hide text. Each of them is printed as a visible escape such as `\u{202e}`. Joiners, variation
//! selectors and the tags of the subdivision flags stay where they are part of an emoji or a word
//! (a woman technologist, a red heart, the flag of Scotland), as the server accepts them in names.
//! A name longer than any the server accepts is cut off.

/// Characters of a name shown; the server allows 32 for a display name and 60 for a challenge.
const NAME_MAX: usize = 64;
/// Characters of an e-mail address shown (the longest one there is).
const ADDRESS_MAX: usize = 254;

/// A display name or a challenge name as the CLI prints it.
pub fn name(text: &str) -> String {
    shown(text, NAME_MAX)
}

/// An e-mail address (or an id) from the server as the CLI prints it.
pub fn address(text: &str) -> String {
    shown(text, ADDRESS_MAX)
}

/// `text` with every hidden character escaped, at most `max` characters long (an escape counts
/// with its length), then "…" if something was left out.
fn shown(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut length = 0;
    let mut i = 0;
    while i < chars.len() {
        let kept = ordinary(&chars, i);
        let part = if kept == 0 {
            format!("\\u{{{:x}}}", chars[i] as u32)
        } else {
            chars[i..i + kept].iter().collect()
        };
        let part_length = part.chars().count();
        if length + part_length > max {
            out.push('…');
            break;
        }
        out.push_str(&part);
        length += part_length;
        i += kept.max(1);
    }
    out
}

/// How many characters from `chars[i]` on print as they are: 0 when `chars[i]` is to be escaped,
/// more than one for a flag with its tags.
fn ordinary(chars: &[char], i: usize) -> usize {
    let joins = |at: Option<&char>| at.is_some_and(|c| !c.is_whitespace() && !hidden(*c));
    match chars[i] {
        // Zero-width non-joiner and joiner: between two characters that show (an emoji sequence,
        // Persian or Indic spelling).
        '\u{200C}' | '\u{200D}' => {
            usize::from(i > 0 && joins(chars.get(i - 1)) && joins(chars.get(i + 1)))
        }
        // A variation selector right after a character that shows (the red heart emoji), but not
        // a second one.
        '\u{FE00}'..='\u{FE0F}' => usize::from(i > 0 && visible(chars[i - 1])),
        // A black flag with the tag letters of a flag that systems show and the cancel tag.
        '\u{1F3F4}' => {
            let tags = chars[i + 1..]
                .iter()
                .take_while(|c| ('\u{E0020}'..='\u{E007E}').contains(*c))
                .count();
            let letters: String = chars[i + 1..i + 1 + tags]
                .iter()
                .filter_map(|c| char::from_u32(*c as u32 - 0xE0000))
                .collect();
            if tags > 0
                && SHOWN_FLAGS.contains(&letters.as_str())
                && chars.get(i + 1 + tags) == Some(&'\u{E007F}')
            {
                tags + 2
            } else {
                1
            }
        }
        c => usize::from(!hidden(c)),
    }
}

/// The subdivision flags that Unicode recommends (England, Scotland, Wales); other tag sequences
/// show as a plain 🏴 and their tags as nothing.
const SHOWN_FLAGS: [&str; 3] = ["gbeng", "gbsct", "gbwls"];

/// Control characters, line and paragraph separators, bidi controls, the other format characters
/// (Unicode category Cf), fillers that show nothing, and the ideographic variation selectors.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{0600}'..='\u{0605}'
                | '\u{061C}'
                | '\u{06DD}'
                | '\u{070F}'
                | '\u{0890}'..='\u{0891}'
                | '\u{08E2}'
                | '\u{115F}'..='\u{1160}'
                | '\u{17B4}'..='\u{17B5}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{110BD}'
                | '\u{110CD}'
                | '\u{13430}'..='\u{1343F}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E007F}'
                | '\u{E0100}'..='\u{E01EF}'
        )
}

/// A character that shows on its own: not blank, not hidden, and no combining mark or selector.
fn visible(c: char) -> bool {
    !c.is_whitespace()
        && !hidden(c)
        && !matches!(
            c,
            '\u{0300}'..='\u{036F}'
                | '\u{1AB0}'..='\u{1AFF}'
                | '\u{1DC0}'..='\u{1DFF}'
                | '\u{20D0}'..='\u{20FF}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FE20}'..='\u{FE2F}'
        )
}

#[cfg(test)]
mod tests {
    use super::{address, name};

    #[test]
    fn ordinary_names_print_as_they_are() {
        for text in [
            "Ada Lovelace",
            "Zoë Ångström",
            "李小龙",
            "محمد",
            "Ünal 👩\u{200d}💻",
            "❤\u{fe0f} Mia",
            "🏳\u{fe0f}\u{200d}🌈",
            "🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f} Ewan",
            "می\u{200c}خواهم",
            "C:\\Users",
        ] {
            assert_eq!(name(text), text);
        }
    }

    #[test]
    fn controls_bidi_and_invisible_characters_become_visible_escapes() {
        // C0: an escape sequence that would clear the screen, a bell, a line break.
        assert_eq!(name("Ada\u{1b}[2J\u{7}"), "Ada\\u{1b}[2J\\u{7}");
        assert_eq!(name("Ada\nBob\r\t"), "Ada\\u{a}Bob\\u{d}\\u{9}");
        // DEL and C1 (a single-byte CSI).
        assert_eq!(name("A\u{7f}B\u{9b}31m"), "A\\u{7f}B\\u{9b}31m");
        // Every bidi control.
        for c in [
            '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
            '\u{2068}', '\u{2069}', '\u{200E}', '\u{200F}', '\u{061C}',
        ] {
            assert_eq!(
                name(&format!("Ada{c}x")),
                format!("Ada\\u{{{:x}}}x", c as u32)
            );
        }
        assert_eq!(name("Ada\u{202e}gnp.exe"), "Ada\\u{202e}gnp.exe");
        // Invisible format characters and fillers.
        assert_eq!(
            name("A\u{200b}d\u{feff}a\u{2060}\u{ad}\u{3164}"),
            "A\\u{200b}d\\u{feff}a\\u{2060}\\u{ad}\\u{3164}"
        );
        // A line separator, tags outside a flag, an ideographic variation selector.
        assert_eq!(name("A\u{2028}B"), "A\\u{2028}B");
        assert_eq!(name("Ada\u{e0041}"), "Ada\\u{e0041}");
        assert_eq!(name("葛\u{e0100}"), "葛\\u{e0100}");
    }

    #[test]
    fn joiners_and_selectors_out_of_place_are_escaped_too() {
        assert_eq!(name("\u{200d}Ada"), "\\u{200d}Ada");
        assert_eq!(name("Ada\u{200d}"), "Ada\\u{200d}");
        assert_eq!(name("Ada \u{200c}Bob"), "Ada \\u{200c}Bob");
        assert_eq!(name("A\u{200d}\u{200d}B"), "A\\u{200d}\\u{200d}B");
        assert_eq!(name("\u{fe0f}Ada"), "\\u{fe0f}Ada");
        assert_eq!(name("❤\u{fe0f}\u{fe0f}"), "❤\u{fe0f}\\u{fe0f}");
        // A flag whose tags no system shows: the flag stays, its hidden letters do not.
        assert_eq!(
            name("🏴\u{e0078}\u{e0079}\u{e007f}"),
            "🏴\\u{e0078}\\u{e0079}\\u{e007f}"
        );
    }

    #[test]
    fn absurdly_long_names_are_cut_off() {
        let long = "x".repeat(10_000);
        let shown = name(&long);
        assert_eq!(shown.chars().count(), 65);
        assert!(shown.ends_with("x…"));
        assert_eq!(name(&"y".repeat(64)), "y".repeat(64));
        // Escapes count with their length, so hidden characters cannot make it long either.
        let hidden = name(&"\u{202e}".repeat(1_000));
        assert!(hidden.chars().count() <= 65, "{hidden}");
        assert!(hidden.starts_with("\\u{202e}\\u{202e}") && hidden.ends_with('…'));
        // An escape is never cut in half.
        assert!(!name(&format!("{}\u{202e}", "z".repeat(60))).contains("\\u{2…"));
        let at = format!("{}@example.org", "a".repeat(200));
        assert_eq!(address(&at), at);
        assert_eq!(address(&"a".repeat(300)).chars().count(), 255);
    }
}
