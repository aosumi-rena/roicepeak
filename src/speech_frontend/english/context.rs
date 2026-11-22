use super::ascii_form;

pub(super) fn pronunciation(word: &str, before: &str, after: &str) -> Option<&'static str> {
    if !matches!(
        word,
        "read"
            | "use"
            | "lead"
            | "record"
            | "wind"
            | "live"
            | "project"
            | "object"
            | "close"
            | "permit"
            | "permits"
    ) {
        return None;
    }
    let (rest, previous) = preceding_word(before);
    let (_, earlier) = preceding_word(rest);
    let following = folded(
        after
            .trim_start()
            .split(word_boundary)
            .next()
            .unwrap_or_default(),
    );
    let previous = previous.as_str();
    let following = following.as_str();
    let subject = matches!(previous, "i" | "you" | "he" | "she" | "it" | "we" | "they");
    let verb = auxiliary(previous)
        || (subject && auxiliary(&earlier))
        || previous == "please"
        || (previous == "to"
            && matches!(
                earlier.as_str(),
                "want" | "need" | "try" | "learn" | "how" | "able" | "going" | "ready" | "time"
            ));
    let clause_start = before.trim_end_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '"' | '“' | '\'' | '‘' | '(')
    });
    let at_start = clause_start.is_empty() || clause_start.ends_with(['.', '!', '?']);
    let determiner = matches!(
        previous,
        "a" | "an" | "the" | "my" | "your" | "his" | "her" | "our" | "their" | "this" | "that"
    ) || previous.ends_with("'s");
    match word {
        "read" if verb || at_start => Some("R IY1 D"),
        "use" if verb || (at_start && following != "of") => Some("Y UW1 Z"),
        "lead" if verb => Some("L IY1 D"),
        "record" if determiner => Some("R EH1 K ER0 D"),
        "wind" if determiner && !after.starts_with(['-', '‐']) => Some("W IH1 N D"),
        "live"
            if (subject || verb)
                && matches!(
                    following,
                    "in" | "at"
                        | "on"
                        | "with"
                        | "near"
                        | "here"
                        | "there"
                        | "nearby"
                        | "alone"
                        | "abroad"
                ) =>
        {
            Some("L IH1 V")
        }
        "project" if verb || (subject && matches!(following, "a" | "an" | "the")) => {
            Some("P R AH0 JH EH1 K T")
        }
        "object" if verb || (subject && following == "to") => Some("AH0 B JH EH1 K T"),
        "close"
            if verb
                || ((at_start || subject)
                    && matches!(
                        following,
                        "the" | "a" | "an" | "it" | "them" | "this" | "that"
                    )) =>
        {
            Some("K L OW1 Z")
        }
        "permit" if determiner => Some("P ER1 M IH2 T"),
        "permits" if determiner => Some("P ER1 M IH2 T S"),
        _ => None,
    }
}

fn auxiliary(word: &str) -> bool {
    matches!(
        word,
        "will"
            | "would"
            | "shall"
            | "should"
            | "can"
            | "could"
            | "may"
            | "might"
            | "must"
            | "do"
            | "does"
            | "did"
            | "don't"
            | "doesn't"
            | "didn't"
            | "can't"
            | "won't"
            | "wouldn't"
            | "couldn't"
            | "shouldn't"
    ) || word.ends_with("'ll")
}

fn folded(word: &str) -> String {
    word.chars()
        .map(|ch| match ascii_form(ch) {
            '’' => '\'',
            value => value.to_ascii_lowercase(),
        })
        .collect()
}

fn word_boundary(ch: char) -> bool {
    !ascii_form(ch).is_ascii_alphabetic() && !matches!(ascii_form(ch), '\'' | '’')
}

fn preceding_word(text: &str) -> (&str, String) {
    let before = text.trim_end();
    let word = before.rsplit(word_boundary).next().unwrap_or_default();
    (&before[..before.len() - word.len()], folded(word))
}
