//! Fidelity gate for the local rewrite: the facts a smoother text must not lose.
//!
//! The 4B model, decoding greedily, sometimes drops or changes a fact while the
//! text still looks clean, and the length/overlap checks of [`crate::validation`]
//! cannot see it. Seen on the real binary and bench (docs/v15/RAPPORT-V15.md):
//! « 192 point 168 point 1 point 1 » became 192.168.68.1, « je ne sais pas »
//! vanished, « deux câbles HDMI » became « des câbles HDMI ». A rejection makes
//! the caller insert the raw transcript, which is faithful by construction.
//! Reasons never carry dictated content: they are logged.

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

pub fn check(raw: &str, cleaned: &str) -> Result<(), &'static str> {
    let raw_numbers = numbers(raw);
    // List markers are formatting the model was asked to add, not numbers.
    let said = numbers(&LIST_MARKER.replace_all(cleaned, ""));
    if !said.is_subset(&raw_numbers) {
        return Err("number_changed");
    }
    // Same values in another order: 192.168.1.50 is not 192.168.50.1.
    let raw_dotted = dotted(raw);
    if dotted(cleaned).iter().any(|seq| !raw_dotted.contains(seq)) {
        return Err("number_changed");
    }
    if !lost_numbers(raw, cleaned).is_empty() {
        return Err("number_lost");
    }
    if !units_added(raw, cleaned).is_empty() {
        return Err("unit_added");
    }
    if has(&HEDGE, raw) && !has(&HEDGE, cleaned) {
        return Err("hedge_lost");
    }
    if !flipped_negations(raw, cleaned).is_empty() {
        return Err("negation_lost");
    }
    if PENDING.is_match(&normalized(raw)) && !PENDING.is_match(&normalized(cleaned)) {
        return Err("pending_lost");
    }
    if kept_negation(raw)
        && !has(&NEGATION, cleaned)
        && !Words::new(cleaned)
            .w
            .iter()
            .any(|w| NEGATING.contains(&w.as_str()))
    {
        return Err("negation_lost");
    }
    if has(&RESTRICTIVE, raw) && !has(&RESTRICTION_KEPT, cleaned) {
        return Err("restriction_lost");
    }
    if !conditions_lost(raw, cleaned).is_empty() {
        return Err("condition_lost");
    }
    let qualified = RETRACTED_QUALIFIER
        .replace_all(&raw.replace('’', "'"), "")
        .into_owned();
    if has(&QUALIFIER, &qualified) && !has(&QUALIFIER, cleaned) {
        return Err("qualifier_lost");
    }
    if has(&DOUBLED_PLUS, raw) && !has(&DOUBLED_PLUS_KEPT, cleaned) {
        return Err("name_changed");
    }
    Ok(())
}

/// The dictated words behind a rejection, for the opt-in local diagnostic only
/// (`crate::diagnostic`): unlike the reason, this quotes the dictation, so it
/// must never be logged.
pub fn explain(raw: &str, cleaned: &str, reason: &str) -> Option<String> {
    let list = |terms: Vec<String>| {
        let terms: BTreeSet<String> = terms.into_iter().collect();
        (!terms.is_empty()).then(|| {
            terms
                .iter()
                .map(|t| format!("« {t} »"))
                .collect::<Vec<_>>()
                .join(", ")
        })
    };
    let found = |pattern: &Regex, text: &str| {
        pattern
            .find_iter(&text.replace('’', "'"))
            .map(|m| m.as_str().to_lowercase())
            .collect::<Vec<_>>()
    };
    let values = |set: BTreeSet<u64>| set.iter().map(u64::to_string).collect::<Vec<_>>();
    let (what, terms) = match reason {
        "number_changed" => ("nombre absent du brut", {
            let raw_dotted = dotted(raw);
            let mut terms = values(&numbers(&LIST_MARKER.replace_all(cleaned, "")) - &numbers(raw));
            terms.extend(
                dotted(cleaned)
                    .into_iter()
                    .filter(|seq| !raw_dotted.contains(seq))
                    .map(|seq| join_dotted(&seq)),
            );
            terms
        }),
        "number_lost" => (
            "nombre du brut absent de la reformulation",
            lost_numbers(raw, cleaned),
        ),
        "unit_added" => ("unité absente du brut", units_added(raw, cleaned)),
        "hedge_lost" => ("incertitude disparue", found(&HEDGE, raw)),
        "negation_lost" => ("négation disparue", {
            let flipped = flipped_negations(raw, cleaned);
            if flipped.is_empty() {
                found(&NEGATION, raw)
            } else {
                flipped
            }
        }),
        "pending_lost" => ("état en attente disparu", found(&PENDING, &normalized(raw))),
        "restriction_lost" => ("condition restrictive disparue", found(&RESTRICTIVE, raw)),
        "condition_lost" => ("condition disparue", conditions_lost(raw, cleaned)),
        "qualifier_lost" => (
            "degré disparu",
            found(
                &QUALIFIER,
                &RETRACTED_QUALIFIER.replace_all(&raw.replace('’', "'"), ""),
            ),
        ),
        "name_changed" => ("nom modifié", found(&DOUBLED_PLUS, raw)),
        _ => return None,
    };
    list(terms).map(|terms| format!("{what} : {terms}"))
}

fn join_dotted(seq: &[u64]) -> String {
    seq.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
}

fn normalized(text: &str) -> String {
    text.to_lowercase().replace('’', "'").replace("n't", " not")
}

/// Lowercase words with their sentence, « n't » read as « not »: what the
/// negation, condition and retraction checks reason on.
struct Words {
    w: Vec<String>,
    sentence: Vec<usize>,
}

impl Words {
    fn new(text: &str) -> Self {
        static SENTENCE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"[.!?;:…]+(?:\s|$)|\n").unwrap());
        let (mut w, mut sentence) = (Vec::new(), Vec::new());
        for (i, s) in SENTENCE.split(&normalized(text)).enumerate() {
            for word in s
                .split(|c: char| !c.is_alphanumeric())
                .filter(|t| !t.is_empty())
            {
                w.push(word.to_string());
                sentence.push(i);
            }
        }
        Self { w, sentence }
    }

    fn is(&self, i: usize, words: &[&str]) -> bool {
        self.w.get(i).is_some_and(|w| words.contains(&w.as_str()))
    }

    /// A word that carries meaning, as a crude stem (« documents » = « document »).
    fn content(&self, i: usize) -> Option<&str> {
        let w = self.w[i].as_str();
        let plain = w.chars().count() >= 3
            && w.chars().all(char::is_alphabetic)
            && !STOP.contains(&w)
            && !NEGATIONS.contains(&w)
            && !CORRECTIONS.contains(&w)
            && word(w).is_none()
            && ordinal(w).is_none();
        plain.then(|| stem(w))
    }

    /// Where the speaker takes something back: the words before a marker
    /// (« neuf heures, enfin non »), and the number after « pas » in « cinq,
    /// pas trois ». Each span is inclusive.
    fn retractions(&self) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        for i in 0..self.w.len() {
            let marker = self.is(i, CORRECTIONS)
                || (self.w[i] == "dire" && i > 0 && self.w[i - 1] == "veux")
                || (self.w[i] == "mean" && i > 0 && self.w[i - 1] == "i");
            if marker {
                spans.push((i.saturating_sub(6), i));
            }
            let spoken = |j: usize| self.w.get(j).and_then(|n| one_value(&[n.as_str()]));
            if self.is(i, &["pas", "not"]) && spoken(i + 1).is_some_and(|v| v > 1) {
                let end = (i + 1..self.w.len())
                    .take_while(|&j| spoken(j).is_some())
                    .last();
                spans.push((i, end.unwrap_or(i + 1)));
            }
        }
        spans
    }
}

fn stem(w: &str) -> &str {
    w.strip_suffix('s')
        .filter(|s| s.chars().count() >= 3)
        .unwrap_or(w)
}

/// `after` also covers the words right after a marker, which restate what was
/// taken back (« du fromage, ah non, pas de fromage »).
fn retracted(spans: &[(usize, usize)], i: usize, after: usize) -> bool {
    spans.iter().any(|&(a, b)| a <= i && i <= b + after)
}

// A speaker who takes something back legitimately loses a number or a « pas »,
// but only there: « rouge, non, du bleu » does not excuse a lost « sert pas ».
const CORRECTIONS: &[&str] = &[
    "non", "enfin", "plutôt", "pardon", "attends", "oups", "trompé", "trompée", "no", "sorry",
    "actually", "rather", "wait",
];
const NEGATIONS: &[&str] = &[
    "pas", "jamais", "aucun", "aucune", "rien", "ni", "not", "never", "no",
];
// Any of these near a word keeps it negated (« ne sert à rien », « sans »).
const NEGATING: &[&str] = &[
    "pas", "jamais", "aucun", "aucune", "rien", "ni", "not", "never", "no", "ne", "n", "sans",
    "without", "non", "nor", "none", "nothing", "nobody", "peu",
];
// Function words of three letters or more: never a negation anchor.
const STOP: &[&str] = &[
    "les", "des", "une", "est", "sont", "était", "être", "été", "suis", "sommes", "êtes", "ont",
    "avait", "avoir", "avons", "avez", "que", "qui", "quoi", "dont", "ça", "cela", "ceci", "cette",
    "ces", "ses", "mes", "tes", "nos", "vos", "leur", "leurs", "son", "mon", "ton", "pour", "par",
    "dans", "sur", "sous", "avec", "chez", "entre", "vers", "mais", "donc", "car", "puis", "alors",
    "aussi", "très", "trop", "peu", "bien", "tout", "tous", "toute", "toutes", "encore", "déjà",
    "toujours", "plus", "moins", "même", "comme", "quand", "lui", "elle", "elles", "ils", "nous",
    "vous", "eux", "moi", "toi", "voilà", "bah", "ben", "euh", "hein", "the", "and", "for", "but",
    "are", "was", "were", "has", "have", "had", "does", "did", "this", "that", "these", "those",
    "with", "from", "into", "its", "yet", "still", "also", "very", "too", "there", "here", "then",
    "than", "they", "them", "you", "your", "our", "his", "her", "she", "him", "can", "could",
    "would", "should", "will", "shall", "may", "might", "must", "been", "being", "again", "any",
    "some", "just", "only", "really",
];

/// Negations whose words come back affirmed: « il sert pas à grand-chose »
/// rewritten « il sert à grand-chose » while other « pas » remain (91661d83).
/// Each negation is pinned to two words around it in its sentence; it is lost
/// when the rewrite has more un-negated pairs of them than the dictation.
/// shortcut: a negation with fewer than two such words (« c'est pas mal ») is
/// only covered by the presence check; anchor further if one slips through.
fn flipped_negations(raw: &str, cleaned: &str) -> Vec<String> {
    let (r, c) = (Words::new(raw), Words::new(cleaned));
    let spans = r.retractions();
    (0..r.w.len())
        .filter(|&p| r.is(p, NEGATIONS) && !retracted(&spans, p, 2) && !rhetorical(&r, p))
        .filter_map(|p| anchors(&r, p))
        .filter(|&(x, y)| affirmed(&c, x, y) > affirmed(&r, x, y))
        .map(|(x, y)| format!("{x} … {y}"))
        .collect()
}

/// « est-ce qu'on pourrait pas améliorer ça » asks for it: « on pourrait
/// améliorer ça » keeps the meaning.
fn rhetorical(t: &Words, p: usize) -> bool {
    (p.saturating_sub(8)..p.saturating_sub(2)).any(|k| {
        t.sentence[k] == t.sentence[p]
            && t.w[k] == "est"
            && t.is(k + 1, &["ce"])
            && t.is(k + 2, &["que", "qu"])
    })
}

fn anchors(t: &Words, p: usize) -> Option<(&str, &str)> {
    let near = |i: usize| t.sentence[i] == t.sentence[p] && t.content(i).is_some();
    let before: Vec<usize> = (p.saturating_sub(6)..p)
        .rev()
        .filter(|&i| near(i))
        .collect();
    let after: Vec<usize> = (p + 1..t.w.len().min(p + 7)).filter(|&i| near(i)).collect();
    let (i, j) = match (before.first(), after.first()) {
        (Some(&b), Some(&a)) if p - b <= 3 && a - p <= 3 => (b, a),
        // Two words on one side only when the first sits within the negation's reach.
        _ if after.len() >= 2 && after[0] - p <= 3 => (after[0], after[1]),
        _ if before.len() >= 2 && p - before[0] <= 3 => (before[1], before[0]),
        _ => return None,
    };
    let (x, y) = (t.content(i)?, t.content(j)?);
    (x != y).then_some((x, y))
}

/// Pairs « x … y » (up to four words apart) with no negating word around them.
fn affirmed(t: &Words, x: &str, y: &str) -> usize {
    let n = t.w.len();
    (0..n)
        .filter(|&i| stem(&t.w[i]) == x)
        .flat_map(|i| (i + 1..n.min(i + 5)).map(move |j| (i, j)))
        .filter(|&(i, j)| t.sentence[j] == t.sentence[i] && stem(&t.w[j]) == y)
        .filter(|&(i, j)| {
            !(i.saturating_sub(5)..n.min(j + 3))
                .any(|k| t.sentence[k] == t.sentence[i] && t.is(k, NEGATING))
        })
        .count()
}

/// A negation said and not taken back, for the presence check.
fn kept_negation(raw: &str) -> bool {
    let r = Words::new(raw);
    let spans = r.retractions();
    (0..r.w.len()).any(|p| r.is(p, NEGATIONS) && !retracted(&spans, p, 2) && !rhetorical(&r, p))
}

/// Raw numbers the rewrite dropped, other than one the speaker took back: a
/// value, or a version whose last segment vanished (1.7.1 → 1.7, 05ee43b7).
fn lost_numbers(raw: &str, cleaned: &str) -> Vec<String> {
    let r = Words::new(raw);
    let taken_back: BTreeSet<u64> = r
        .retractions()
        .iter()
        .flat_map(|&(a, b)| numbers(&r.w[a..=b].join(" ")))
        .collect();
    // A spoken « deux points » may be the colon (« C deux-points antislash ») or
    // a quantity: its 2 is allowed in the output, not required.
    // 0 and 1 are mostly articles (« un switch » → « le switch »).
    let kept = numbers(cleaned);
    let mut lost: Vec<String> = numbers(&COLON.replace_all(raw, " : "))
        .into_iter()
        .filter(|n| *n > 1 && !kept.contains(n) && !taken_back.contains(n))
        .map(|n| n.to_string())
        .collect();
    let written = dotted(cleaned);
    lost.extend(
        dotted(raw)
            .into_iter()
            .filter(|seq| seq.len() >= 3 && !seq.iter().any(|n| taken_back.contains(n)))
            .filter(|seq| {
                !written
                    .iter()
                    .any(|w| w.windows(seq.len()).any(|s| s == seq))
            })
            .map(|seq| join_dotted(&seq)),
    );
    lost
}

/// A plain « si » / « if » whose condition vanished: « il n'y a pas l'onglet
/// Correction s'il n'y a pas de correction » became a concession (70355dc8).
/// The condition is pinned to the first word it governs; it is lost when the
/// rewrite keeps that word with no conditional wording before it, or drops
/// both the word and every conditional wording. Returns those words.
fn conditions_lost(raw: &str, cleaned: &str) -> Vec<String> {
    let (r, c) = (Words::new(raw), Words::new(cleaned));
    let spans = r.retractions();
    let any_kept = (0..c.w.len()).any(|k| conditional(&c, k));
    let mut lost = Vec::new();
    for i in (0..r.w.len()).filter(|&i| condition(&r, i) && !retracted(&spans, i, 2)) {
        let anchor = (i + 1..r.w.len().min(i + 9))
            .filter(|&j| r.sentence[j] == r.sentence[i])
            .find_map(|j| r.content(j));
        let Some(anchor) = anchor else {
            if !any_kept {
                lost.push(r.w[i].clone());
            }
            continue;
        };
        let seen: Vec<usize> = (0..c.w.len())
            .filter(|&j| stem(&c.w[j]) == anchor)
            .collect();
        let still = seen.iter().any(|&j| {
            (j.saturating_sub(12)..j).any(|k| c.sentence[k] == c.sentence[j] && conditional(&c, k))
        });
        if (seen.is_empty() && !any_kept) || (!seen.is_empty() && !still) {
            lost.push(anchor.to_string());
        }
    }
    lost
}

fn condition(t: &Words, i: usize) -> bool {
    const OPENS: &[&str] = &[
        "je",
        "j",
        "tu",
        "t",
        "il",
        "ils",
        "elle",
        "elles",
        "on",
        "nous",
        "vous",
        "ça",
        "c",
        "ce",
        "cela",
        "le",
        "la",
        "les",
        "l",
        "un",
        "une",
        "des",
        "du",
        "mon",
        "ma",
        "mes",
        "ton",
        "ta",
        "tes",
        "son",
        "sa",
        "ses",
        "notre",
        "nos",
        "votre",
        "vos",
        "leur",
        "leurs",
        "y",
        "personne",
        "rien",
        "aucun",
        "aucune",
        "jamais",
        "quelqu",
        "quelque",
        "possible",
        "besoin",
        "nécessaire",
    ];
    // « je sais pas si », « même si », « I wonder if »: whether, even if, as if.
    // « genre si », « par exemple si »: an illustration, not a condition.
    const NOT_A_CONDITION: &[&str] = &[
        "genre",
        "exemple",
        "sais",
        "sait",
        "savoir",
        "demande",
        "voir",
        "vérifier",
        "regarder",
        "know",
        "wonder",
        "check",
        "see",
        "ask",
        "sure",
        "sûr",
        "même",
        "comme",
        "as",
        "even",
    ];
    let opens = (t.w[i] == "si" && t.is(i + 1, OPENS))
        || (t.w[i] == "s" && t.is(i + 1, &["il", "ils"]))
        || t.w[i] == "if";
    opens
        && !(i.saturating_sub(2)..i)
            .any(|k| t.sentence[k] == t.sentence[i] && t.is(k, NOT_A_CONDITION))
}

/// Any wording that still makes something conditional (« quand », « sans »,
/// « en cas de »), so a rephrased condition passes.
fn conditional(t: &Words, k: usize) -> bool {
    const KEPT: &[&str] = &[
        "si",
        "quand",
        "lorsque",
        "lorsqu",
        "dès",
        "cas",
        "condition",
        "tant",
        "sauf",
        "moins",
        "pourvu",
        "sans",
        "faute",
        "absence",
        "seulement",
        "uniquement",
        "fois",
        "éventuel",
        "éventuelle",
        "éventuels",
        "éventuelles",
        "éventuellement",
        "if",
        "when",
        "whenever",
        "unless",
        "once",
        "provided",
        "case",
        "without",
        "only",
    ];
    t.is(k, KEPT) || (t.w[k] == "s" && t.is(k + 1, &["il", "ils"]))
}

/// Units written in the rewrite that the speaker never said for that number:
/// « vingt de temps » became « vingt minutes » (1.6 field test T12), even with
/// « cinq minutes » said elsewhere (b5a55eaa). Compared by kind and paired with
/// the number just before, so « dix heures » → « 10 h » and « seize gigas » →
/// « 16 Go » pass. Returns the offending words as written in the rewrite.
fn units_added(raw: &str, cleaned: &str) -> Vec<String> {
    // The number words right before the unit (up to three), plus the digit an
    // abbreviation match starts with (« 10 h » matches from its « 0 »). In
    // « 1 400 euros par mois » the 400 belongs to « euros », not to « mois ».
    let number = |text: &str, m: &regex::Match| {
        let head = &text[..m.start()];
        let words: Vec<&str> = head.split_whitespace().collect();
        let start = words.len().saturating_sub(3);
        let start = (start..words.len())
            .rev()
            .take_while(|&i| !numbers(words[i]).is_empty())
            .last()
            .unwrap_or(words.len());
        let mut near = words[start..].join(" ");
        if head.ends_with(char::is_whitespace) {
            near.push(' ');
        }
        near.extend(m.as_str().chars().take_while(char::is_ascii_digit));
        numbers(&near)
    };
    UNITS
        .iter()
        .flat_map(|unit| {
            let said: Vec<BTreeSet<u64>> = unit.find_iter(raw).map(|m| number(raw, &m)).collect();
            let heard: BTreeSet<u64> = said.iter().flatten().copied().collect();
            unit.find_iter(cleaned)
                .filter(|m| {
                    said.is_empty() || {
                        let written = number(cleaned, m);
                        !written.is_empty() && written.is_disjoint(&heard)
                    }
                })
                .map(|m| unit_word(m.as_str()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// An abbreviation match carries its digit and the next character.
fn unit_word(found: &str) -> String {
    let unit = found.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_whitespace());
    match unit.char_indices().last() {
        Some((i, c)) if unit.len() > 1 && !c.is_alphanumeric() && !"%€$".contains(c) => {
            unit[..i].trim_end()
        }
        _ => unit,
    }
    .to_string()
}

// Spelled-out words anywhere; symbols and abbreviations only right after a
// digit (« 5 s », not the « s » of a word). Checked on the 116 bench references
// and the 20 accepted rewrites of the 1.6 field test: no false alarm.
static UNITS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        (r"secondes?|seconds?", r"s|sec"),
        (r"minutes?", r"min|mn"),
        (r"heures?|hours?", r"h"),
        (r"jours?|days?", r"j"),
        (r"semaines?|weeks?", ""),
        (r"mois|months?", ""),
        // Not a bare « an »: it is also the English article.
        (r"ans|années?|years?", ""),
        (r"pour[ -]?cents?|pourcentage|percent", r"%"),
        (r"euros?", r"€|EUR"),
        (r"dollars?", r"\$|USD"),
        (
            r"(?:kilo|méga|mega|giga|téra|tera)(?:octets?|bytes?)|(?:méga|mega|giga|téra|tera)s?|octets?|bytes?",
            r"[KMGT]i?[oB]",
        ),
        (r"(?:kilo|méga|mega|giga)bits?", r"[KMG]bit/s|[KMG]bps|[KMG]b/s"),
        (r"(?:kilo|méga|mega|giga)?hertz", r"[kKMG]?Hz"),
        (r"(?:kilo|centi|milli)?mètres?|(?:kilo|centi|milli)?meters?", r"km|cm|mm|m"),
        // « un kilo deux » is 1,2 kg.
        (r"(?:kilo|milli)?grammes?|(?:kilo|milli)?grams?|kilos?", r"kg|mg|g"),
        (r"volts?|watts?|ampères?", r"[kKm]?[VWA]"),
        (r"pouces?|inch(?:es)?", ""),
        (r"millisecondes?|milliseconds?", r"ms"),
    ]
    .into_iter()
    .map(|(words, abbreviations)| {
        let mut pattern = format!(r"(?i:\b(?:{words})\b)");
        if !abbreviations.is_empty() {
            pattern.push_str(&format!(r"|\d\s?(?:{abbreviations})(?:[^\w]|$)"));
        }
        Regex::new(&pattern).unwrap()
    })
    .collect()
});

// A necessary condition (« uniquement si ») weakened to a plain « si » changes
// what is allowed (1.5.1 field test). A plain « si » is not checked.
static RESTRICTIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:uniquement|seulement|exclusivement) (?:si|s'|après|quand|lorsqu|une fois)|\bque si\b|\bqu'(?:à condition|après|une fois)|\bà condition (?:que|qu'|de|d')|\bsauf (?:si|s')|\bà moins (?:que|qu')|\bpourvu qu|\bonly (?:if|when|after|once)\b|\bunless\b|\bprovided (?:that )?\b|\bexcept (?:if|when)\b|\bas long as\b").unwrap()
});
// Any wording that still restricts, so a rephrased condition passes.
static RESTRICTION_KEPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:uniquement|seulement|exclusivement|que si|qu'à condition|qu'après|qu'une fois|à condition|sauf|à moins|pourvu|only|unless|provided|except|as long as)\b").unwrap()
});
// « pas forcément très puissant » is not « pas très puissant » (1.5.1 field test).
static QUALIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bpas (?:forcément|nécessairement|vraiment|tout à fait|complètement|entièrement|totalement)\b|\bnot (?:necessarily|really|quite|entirely|completely|fully)\b").unwrap()
});
// Right after « enfin » or « non » the same words take something back
// (« enfin pas vraiment le serveur, plutôt l'application »): they may go.
static RETRACTED_QUALIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:enfin|non|pardon|ou plutôt|well|sorry|or rather)\b[ ,]*(?:ce n'est |ce n'était |c'est |c'était |it's |it was )?(?:pas|not) (?:forcément|nécessairement|vraiment|tout à fait|complètement|entièrement|totalement|necessarily|really|quite|entirely|completely|fully)\b").unwrap()
});

static COLON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bdeux[- ]points?\b").unwrap());
static LIST_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*\d+[.)]\s").unwrap());
static HEDGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)peut-être|peut être|\bje pense\b|\bje crois\b|sais pas|pas sûr|pas certain|probablement|sans doute|il me semble|j'imagine|\bi think\b|\bmaybe\b|not sure|n't know|\bprobably\b|\bperhaps\b|\bi guess\b").unwrap()
});
// « pas encore envoyé » is a status: « envoyé » alone is another fact
// (f9d3ea95). Read on [`normalized`] text, where « haven't » is « have not ».
static PENDING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bpas encore\b|\btoujours pas\b|\bnot (?:[\w']+ ){0,3}?yet\b").unwrap()
});
// « Notepad plus plus » is a name, not a repeated word (f9d3ea95).
static DOUBLED_PLUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bplus plus\b").unwrap());
static DOUBLED_PLUS_KEPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bplus plus\b|\+\+").unwrap());
static NEGATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(pas|jamais|aucun|aucune|rien|ni|not|never|no)\b|n't").unwrap()
});

fn has(pattern: &Regex, text: &str) -> bool {
    pattern.is_match(&text.replace('’', "'"))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Unit,
    Tens,
    Hundred,
    Thousand,
}

fn word(w: &str) -> Option<(u64, Kind)> {
    let v = match w {
        "zéro" | "zero" => 0,
        "un" | "une" | "one" => 1,
        "deux" | "two" => 2,
        "trois" | "three" => 3,
        "quatre" | "four" => 4,
        "cinq" | "five" => 5,
        "six" => 6,
        "sept" | "seven" => 7,
        "huit" | "eight" => 8,
        "neuf" | "nine" => 9,
        "dix" | "ten" => 10,
        "onze" | "eleven" => 11,
        "douze" | "twelve" => 12,
        "treize" | "thirteen" => 13,
        "quatorze" | "fourteen" => 14,
        "quinze" | "fifteen" => 15,
        "seize" | "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        "vingt" | "vingts" | "twenty" => return Some((20, Kind::Tens)),
        "trente" | "thirty" => return Some((30, Kind::Tens)),
        "quarante" | "forty" => return Some((40, Kind::Tens)),
        "cinquante" | "fifty" => return Some((50, Kind::Tens)),
        "soixante" | "sixty" => return Some((60, Kind::Tens)),
        "seventy" => return Some((70, Kind::Tens)),
        "eighty" => return Some((80, Kind::Tens)),
        "ninety" => return Some((90, Kind::Tens)),
        "cent" | "cents" | "hundred" => return Some((100, Kind::Hundred)),
        "mille" | "thousand" => return Some((1000, Kind::Thousand)),
        _ => return None,
    };
    Some((v, Kind::Unit))
}

fn ordinal(w: &str) -> Option<u64> {
    Some(match w {
        "premier" | "première" | "first" => 1,
        "second" | "seconde" | "deuxième" => 2,
        "troisième" => 3,
        "quatrième" => 4,
        "cinquième" => 5,
        "sixième" => 6,
        "septième" => 7,
        "huitième" => 8,
        "neuvième" => 9,
        "dixième" => 10,
        _ => return None,
    })
}

/// A spoken number being read, word by word.
#[derive(Default)]
struct Spoken {
    total: u64,
    current: u64,
    last: Option<(u64, Kind)>,
}

impl Spoken {
    fn flush(&mut self, found: &mut BTreeSet<u64>) {
        if self.last.is_some() {
            found.insert(self.total + self.current);
        }
        *self = Self::default();
    }

    fn push(&mut self, value: u64, kind: Kind, hyphenated: bool, found: &mut BTreeSet<u64>) {
        let joins = match (kind, self.last) {
            (_, None) => true,
            (Kind::Unit, Some((_, Kind::Tens | Kind::Hundred | Kind::Thousand))) => true,
            // dix-sept, soixante-dix-neuf
            (Kind::Unit, Some((10, Kind::Unit))) => hyphenated && (7..=9).contains(&value),
            // quatre-vingt(s)
            (Kind::Tens, Some((4, Kind::Unit))) => value == 20,
            (Kind::Tens, Some((_, Kind::Hundred | Kind::Thousand))) => true,
            // deux mille cent = 2100
            (Kind::Hundred, Some((_, Kind::Unit | Kind::Tens | Kind::Thousand))) => true,
            (Kind::Thousand, Some(_)) => true,
            _ => false,
        };
        if !joins {
            self.flush(found);
        }
        match kind {
            Kind::Tens if matches!(self.last, Some((4, Kind::Unit))) => self.current += 76,
            Kind::Hundred => self.current = self.current.max(1) * 100,
            Kind::Thousand => {
                self.total += self.current.max(1) * 1000;
                self.current = 0;
            }
            _ => self.current += value,
        }
        self.last = Some((value, kind));
    }
}

/// Every number in the text, digits or spoken French/English words
/// (« cent quatre-vingt-douze » = 192), as a set of values.
pub fn numbers(text: &str) -> BTreeSet<u64> {
    static THOUSANDS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d{1,3})[ \u{a0}\u{202f}](\d{3})\b").unwrap());
    // « vingt pour cent » is 20 %, not 20 and 100. Only right after a number:
    // « une table pour cent personnes » keeps its 100.
    static PERCENT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d|\b(?:un|une|deux|trois|quatre|cinq|six|sept|huit|neuf|dix|onze|douze|treize|quatorze|quinze|seize|vingts?|trente|quarante|cinquante|soixante|cents?|mille))[ \u{a0}-]+pour[ -]?cents?\b").unwrap()
    });
    let lowered = THOUSANDS
        .replace_all(&text.to_lowercase(), "$1$2")
        .into_owned();
    let lowered = PERCENT.replace_all(&lowered, "$1 %");
    let mut found = BTreeSet::new();
    let mut spoken = Spoken::default();
    // Hyphens join a compound (« quatre-vingt-douze »); any other separator,
    // a comma or « point », ends the number.
    for token in lowered.split(|c: char| !(c.is_alphanumeric() || c == '-')) {
        for (i, part) in token.split('-').enumerate() {
            let digits = part
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(part.len());
            if digits > 0 {
                spoken.flush(&mut found);
                found.extend(part[..digits].parse::<u64>().ok());
            } else if part == "et" && matches!(spoken.last, Some((_, Kind::Tens))) {
                continue;
            } else if let Some(n) = ordinal(part) {
                spoken.flush(&mut found);
                found.insert(n);
            } else if let Some((value, kind)) = word(part) {
                spoken.push(value, kind, i > 0, &mut found);
            } else {
                spoken.flush(&mut found);
            }
        }
    }
    spoken.flush(&mut found);
    found
}

/// The single value `words` spell (« cent soixante-huit », « 168 »), if they
/// spell exactly one.
fn one_value(words: &[&str]) -> Option<u64> {
    if let [w] = words
        && w.bytes().all(|b| b.is_ascii_digit())
    {
        return w.parse().ok();
    }
    let mut spoken = Spoken::default();
    let mut found = BTreeSet::new();
    for w in words {
        for (i, part) in w.split('-').enumerate() {
            if part == "et" && matches!(spoken.last, Some((_, Kind::Tens))) {
                continue;
            }
            let (value, kind) = word(part)?;
            spoken.push(value, kind, i > 0, &mut found);
            if !found.is_empty() {
                return None;
            }
        }
    }
    spoken.flush(&mut found);
    found.pop_first()
}

/// Numbers joined by « . » or a spoken « point », in order: versions (1.7.1)
/// and addresses (192.168.1.50), whose order [`numbers`] cannot see. A « point »
/// without a number on each side (« qu'un point à discuter ») joins nothing.
/// shortcut: « , » and « virgule » are read as decimals, not joins; add them if
/// a version dictated that way shows up.
fn dotted(text: &str) -> Vec<Vec<u64>> {
    static DOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d)\.(\d)").unwrap());
    static GLUED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\p{L})(\d)|(\d)(\p{L})").unwrap());
    let mut text = text.to_lowercase();
    // Twice: the matches of « 1.7.1 » overlap.
    for _ in 0..2 {
        text = DOT.replace_all(&text, "$1 point $2").into_owned();
    }
    let text = GLUED.replace_all(&text, "$1$3 $2$4");
    let tokens: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|t| !t.is_empty())
        .collect();
    let segments: Vec<&[&str]> = tokens.split(|t| *t == "point").collect();
    let tail = |s: &[&str]| (0..s.len()).find_map(|i| one_value(&s[i..]));
    let head = |s: &[&str]| (1..=s.len()).rev().find_map(|j| one_value(&s[..j]));
    let mut found = Vec::new();
    let mut k = 0;
    while k + 1 < segments.len() {
        let (Some(first), Some(second)) = (tail(segments[k]), head(segments[k + 1])) else {
            k += 1;
            continue;
        };
        let mut seq = vec![first, second];
        k += 1;
        while k + 1 < segments.len() && one_value(segments[k]).is_some() {
            let Some(next) = head(segments[k + 1]) else {
                break;
            };
            seq.push(next);
            k += 1;
        }
        found.push(seq);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_numbers_keep_their_order() {
        let none = Vec::<Vec<u64>>::new();
        assert_eq!(
            dotted(
                "Mon adresse est cent quatre-vingt-douze point cent soixante-huit point un point cinquante."
            ),
            vec![vec![192, 168, 1, 50]]
        );
        assert_eq!(
            dotted("Mon adresse est 192.168.1.50."),
            vec![vec![192, 168, 1, 50]]
        );
        assert_eq!(
            dotted("Jot un point sept point un, puis la v2.0 et Python 3 point 12."),
            vec![vec![1, 7, 1], vec![2, 0], vec![3, 12]]
        );
        assert_eq!(dotted("Il n'y a qu'un point à discuter."), none);
        assert_eq!(
            dotted("Voici mon premier point et mon deuxième point. Un point important."),
            none
        );
        assert_eq!(dotted("1. Branche le câble.\n2. Ouvre."), none);
        assert_eq!(
            dotted("la un point six et deux deux point trois"),
            vec![vec![1, 6], vec![2, 3]]
        );
    }

    #[test]
    fn spoken_numbers_written_in_digits_pass_and_altered_ones_do_not() {
        let ip = "Mon adresse est cent quatre-vingt-douze point cent soixante-huit point un point cinquante.";
        let ok = [
            (
                "J'utilise la version un point sept de Jot.",
                "J'utilise la version 1.7 de Jot.",
            ),
            (
                "L'ancienne vidéo parle de Jot un point cinq.",
                "L'ancienne vidéo parle de Jot 1.5.",
            ),
            (
                "Il faudra publier Jot un point sept point un.",
                "Il faudra publier Jot 1.7.1.",
            ),
            (
                "Nous sommes en deux mille vingt-six.",
                "Nous sommes en 2026.",
            ),
            (
                "Le prix est de vingt dollars.",
                "Le prix est de 20 dollars.",
            ),
            ("Le prix est de vingt dollars.", "Le prix est de 20 $."),
            (
                "Le serveur a seize gigas de RAM et quatre processeurs virtuels.",
                "Le serveur a 16 Go de RAM et 4 processeurs virtuels.",
            ),
            (ip, "Mon adresse est 192.168.1.50."),
            (
                "Ce n'est pas la version un point sept, non, c'est la version un point cinq.",
                "Ce n'est pas la version 1.7, c'est la version 1.5.",
            ),
            (
                "Ce n'est pas la version un point sept, non, c'est la version un point cinq.",
                "C'est la version 1.5.",
            ),
            (
                "Il n'y a qu'un point à discuter.",
                "Il n'y a qu'un point à discuter.",
            ),
            (
                "Voici mon premier point et mon deuxième point.",
                "Voici mon premier point et mon deuxième point.",
            ),
        ];
        for (raw, cleaned) in ok {
            assert_eq!(check(raw, cleaned), Ok(()), "{cleaned}");
        }
        let bad = [
            (
                "J'utilise la version un point sept de Jot.",
                "J'utilise la version 1.8 de Jot.",
            ),
            (
                "J'utilise la version un point sept de Jot.",
                "J'utilise la version 7.1 de Jot.",
            ),
            (
                "Le prix est de vingt dollars.",
                "Le prix est de 200 dollars.",
            ),
            (ip, "Mon adresse est 192.168.50.1."),
            (
                "Mon adresse est 192.168.1.50.",
                "Mon adresse est 192.168.50.1.",
            ),
            (
                "Il faudra publier Jot un point sept point un.",
                "Il faudra publier Jot 1.1.7.",
            ),
        ];
        for (raw, cleaned) in bad {
            assert_eq!(check(raw, cleaned), Err("number_changed"), "{cleaned}");
        }
        assert_eq!(
            explain(
                "Mon adresse est 192.168.1.50.",
                "Mon adresse est 192.168.50.1.",
                "number_changed"
            )
            .as_deref(),
            Some("nombre absent du brut : « 192.168.50.1 »")
        );
    }

    fn set(values: &[u64]) -> BTreeSet<u64> {
        values.iter().copied().collect()
    }

    #[test]
    fn spoken_numbers_parse_as_one_value() {
        assert_eq!(
            numbers("cent quatre-vingt-douze point cent soixante-huit point un point un"),
            set(&[192, 168, 1])
        );
        assert_eq!(numbers("192.168.68.1"), set(&[192, 168, 68, 1]));
        assert_eq!(
            numbers("deux mille vingt-six, soixante et onze, dix-sept"),
            set(&[2026, 71, 17])
        );
        assert_eq!(
            numbers("trois cents euros et quatre-vingts pages"),
            set(&[300, 80])
        );
        assert_eq!(numbers("un, deux, trois"), set(&[1, 2, 3]));
        assert_eq!(numbers("1 500 personnes à 10 h"), set(&[1500, 10]));
        assert_eq!(numbers("le deuxième devis, la 3e page"), set(&[2, 3]));
        assert_eq!(numbers("twenty one cats, a hundred dogs"), set(&[21, 100]));
    }

    #[test]
    fn a_changed_or_lost_fact_is_rejected() {
        let ip = "Pour configurer le routeur, d'abord tu branches le câble sur le port quatre, ensuite tu ouvres l'adresse cent quatre-vingt-douze point cent soixante-huit point un point un, puis tu changes le mot de passe par défaut.";
        let bad = "Pour configurer le routeur :\n1. Branche le câble sur le port quatre.\n2. Ouvre l'adresse 192.168.68.1.\n3. Change le mot de passe par défaut.";
        assert_eq!(check(ip, bad), Err("number_changed"));
        assert_eq!(
            check(ip, &bad.replace("192.168.68.1", "192.168.1.1")),
            Ok(())
        );

        let hdmi = "À acheter : des câbles HDMI, un switch huit ports, des câbles HDMI, voilà, deux câbles HDMI en tout, et une multiprise.";
        assert_eq!(
            check(
                hdmi,
                "À acheter :\n- Des câbles HDMI.\n- Un switch huit ports.\n- Une multiprise."
            ),
            Err("number_lost")
        );
        assert_eq!(
            check(
                hdmi,
                "À acheter :\n- Deux câbles HDMI.\n- Un switch 8 ports.\n- Une multiprise."
            ),
            Ok(())
        );

        let unsure = "Soit on fait la mise à jour d'abord, soit on change le disque d'abord, je ne sais pas, on verra demain avec Sami.";
        assert_eq!(
            check(
                unsure,
                "Soit on fait la mise à jour d'abord, soit on change le disque d'abord : on verra demain avec Sami."
            ),
            Err("hedge_lost")
        );
        assert_eq!(
            check(
                "Ne supprime pas le dossier backup.",
                "Supprime le dossier backup."
            ),
            Err("negation_lost")
        );
        // 1.5.1 field tests: a necessary condition and a degree qualifier.
        let only_if = "Si les sauvegardes sont terminées et uniquement si les sauvegardes sont terminées, on pourra intervenir demain.";
        assert_eq!(
            check(
                only_if,
                "Si les sauvegardes sont terminées, on pourra intervenir demain."
            ),
            Err("restriction_lost")
        );
        assert_eq!(
            check(
                "Ship it on Friday unless Paul objects.",
                "Ship it on Friday, and ask Paul."
            ),
            Err("restriction_lost")
        );
        assert_eq!(
            check(
                "Mon ordinateur n'est pas forcément très puissant.",
                "Mon ordinateur n'est pas très puissant."
            ),
            Err("qualifier_lost")
        );
    }

    #[test]
    fn rephrased_conditions_and_retractions_pass() {
        let only_if = "Si les sauvegardes sont terminées et uniquement si les sauvegardes sont terminées, on pourra intervenir demain.";
        for kept in [
            "Uniquement si les sauvegardes sont terminées, on pourra intervenir demain.",
            "On ne pourra intervenir demain que si les sauvegardes sont terminées.",
            "Si, et seulement si, les sauvegardes sont terminées, on pourra intervenir demain.",
            "On pourra intervenir demain, à condition que les sauvegardes soient terminées.",
        ] {
            assert_eq!(check(only_if, kept), Ok(()), "{kept}");
        }
        // A plain « si » is not a restriction to keep word for word.
        assert_eq!(
            check(
                "Euh, si tu as le temps, regarde le rapport.",
                "Si tu as le temps, regarde le rapport."
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "Le portable n'est pas forcément plus rapide.",
                "Le portable n'est pas nécessairement plus rapide."
            ),
            Ok(())
        );
        // « enfin pas vraiment le serveur » is the retraction itself: it goes.
        assert_eq!(
            check(
                "Le serveur était lent, enfin pas vraiment le serveur, c'était plutôt l'application qui répondait lentement.",
                "L'application répondait lentement."
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "The server was slow, well, not really the server, I mean the app.",
                "The app was slow."
            ),
            Ok(())
        );
    }

    #[test]
    fn an_invented_unit_is_rejected_and_a_rewritten_one_passes() {
        // 1.6 field test T12: the ASR dropped a word, the model filled it in.
        let t12 = "On a peut-être gagné, on a peut-être gagné à peu près vingt de temps, sans pouvoir encore confirmer ce chiffre.";
        let invented =
            "On a peut-être gagné environ vingt minutes, sans pouvoir encore confirmer ce chiffre.";
        assert_eq!(check(t12, invented), Err("unit_added"));
        assert_eq!(
            explain(t12, invented, "unit_added").as_deref(),
            Some("unité absente du brut : « minutes »")
        );
        assert_eq!(
            check(t12, "On a peut-être gagné à peu près 20 %."),
            Err("unit_added")
        );
        for (raw, cleaned) in [
            ("Je viens à dix heures.", "Je viens à 10 h."),
            ("Il a seize gigas de RAM.", "Il a 16 Go de RAM."),
            ("Le repas coûte dix-huit euros.", "Le repas coûte 18 €."),
            ("Il pèse un kilo deux.", "Il pèse 1,2 kg."),
            ("Attends cinq secondes.", "Attends 5 s."),
            // Not units: a word starting with one, and English « an ».
            (
                "La machine a 4 vCPU et un 5G.",
                "La machine a 4 vCPU et un 5G.",
            ),
            ("It is an option.", "It's an option."),
        ] {
            assert_eq!(check(raw, cleaned), Ok(()), "{cleaned}");
        }
        // Formerly refused: « pour cent » and « mille cent » read as an extra 100.
        for (raw, cleaned) in [
            (
                "Il y a vingt pour cent de remise.",
                "Il y a 20 % de remise.",
            ),
            ("Ça coûte deux mille cent euros.", "Ça coûte 2 100 €."),
            ("Ça coûte deux mille cent euros.", "Ça coûte 2100 €."),
            ("On vise quatre-vingt-dix pour cent.", "On vise 90 %."),
            ("Une remise de cinq pour-cent.", "Une remise de 5 %."),
            ("Cent pour cent sûr.", "100 % sûr."),
            ("Mille cent personnes.", "1 100 personnes."),
        ] {
            assert_eq!(check(raw, cleaned), Ok(()), "{cleaned}");
        }
        // The same words still guard the facts.
        for (raw, cleaned, reason) in [
            (
                "Il y a vingt pour cent de remise.",
                "Il y a 25 % de remise.",
                "number_changed",
            ),
            (
                "Il y a vingt pour cent de remise.",
                "Il y a de la remise.",
                "number_lost",
            ),
            (
                "Ça coûte deux mille cent euros.",
                "Ça coûte 2 000 €.",
                "number_changed",
            ),
            (
                "Ça coûte deux mille cent euros.",
                "Ça coûte 2 000 € et 100 €.",
                "number_changed",
            ),
            (
                "Une table pour cent personnes.",
                "Une table pour des personnes.",
                "number_lost",
            ),
            ("Deux mille, cent fois.", "2100 fois.", "number_changed"),
        ] {
            assert_eq!(check(raw, cleaned), Err(reason), "{cleaned}");
        }
    }

    #[test]
    fn explain_quotes_what_the_check_missed() {
        assert_eq!(
            explain(
                "Je pense que ça marche, peut-être.",
                "Ça marche.",
                "hedge_lost"
            )
            .as_deref(),
            Some("incertitude disparue : « je pense », « peut-être »")
        );
        assert_eq!(
            explain(
                "Ce sera uniquement après la validation.",
                "Ce sera après la validation.",
                "restriction_lost"
            )
            .as_deref(),
            Some("condition restrictive disparue : « uniquement après »")
        );
        assert_eq!(
            explain("Il faut 3 postes.", "Il faut 5 postes.", "number_changed").as_deref(),
            Some("nombre absent du brut : « 5 »")
        );
        assert_eq!(explain("a", "b", "length ratio"), None);
    }

    // Field audit of 16 November 2026 (docs/qualite/REX-FIDELITE-NOV-2026.md):
    // synthetic, anonymised versions of the rewrites the gate let through.
    #[test]
    fn rex_nov_2026_a_negation_lost_beside_others_is_rejected() {
        // 91661d83: other « pas » remain in the rewrite.
        let raw = "Le coffre ne s'ouvre pas, c'est pas terrible. Le levier, là où il est placé, il sert pas à grand-chose. Il n'y a pas encore de son.";
        assert_eq!(
            check(
                raw,
                "Le coffre ne s'ouvre pas, ce n'est pas terrible. Le levier, là où il est placé, sert à grand-chose. Il n'y a pas encore de son."
            ),
            Err("negation_lost")
        );
        assert_eq!(
            explain(
                raw,
                "Le levier sert à grand-chose. Il n'y a pas encore de son.",
                "negation_lost"
            )
            .as_deref(),
            Some("négation disparue : « sert … grand »")
        );
        // A self-correction elsewhere no longer turns the negation check off.
        assert_eq!(
            check(
                "Le levier sert pas à grand-chose. Pour la couleur, mets du rouge, non, du bleu.",
                "Le levier sert à grand-chose. Pour la couleur, mets du bleu."
            ),
            Err("negation_lost")
        );
        // A filler « attends » is not a retraction of what follows.
        assert_eq!(
            check(
                "Attends, j'ai pas encore envoyé le devis au client. Par contre l'impression marche pas sur la vieille imprimante.",
                "J'ai envoyé le devis au client. Par contre, l'impression ne marche pas sur la vieille imprimante."
            ),
            Err("negation_lost")
        );
        assert_eq!(
            check(
                "Le bouton d'export ne fonctionne jamais.",
                "Le bouton d'export fonctionne."
            ),
            Err("negation_lost")
        );
        assert_eq!(
            check("It doesn't work on the laptop.", "It works on the laptop."),
            Err("negation_lost")
        );
    }

    #[test]
    fn rex_nov_2026_negations_kept_or_retracted_pass() {
        for (raw, cleaned) in [
            ("J'ai pas reçu la facture.", "Je n'ai pas reçu la facture."),
            (
                "Y a pas de réseau dans la salle.",
                "Il n'y a pas de réseau dans la salle.",
            ),
            (
                "Le levier, il sert pas à grand-chose.",
                "Le levier ne sert pas à grand-chose.",
            ),
            (
                "Le levier, il sert pas à grand-chose.",
                "Le levier ne sert à rien.",
            ),
            ("Trois pommes, non, cinq pommes.", "Cinq pommes."),
            (
                "Le serveur était lent, enfin non, c'est l'application.",
                "L'application était lente.",
            ),
            (
                "Ce n'est pas le serveur qui est lent, enfin non, c'est l'application qui est lente.",
                "L'application est lente.",
            ),
            // The same words, negated once and affirmed once, both kept.
            (
                "Je veux pas de sucre dans le café. Je veux du sucre dans le thé.",
                "Je ne veux pas de sucre dans le café, mais j'en veux dans le thé.",
            ),
            (
                "Je veux pas de sucre dans le café. Je veux du sucre dans le thé.",
                "Je ne veux pas de sucre dans le café. Je veux du sucre dans le thé.",
            ),
            (
                "Le bouton ne marche pas. Non pardon, le bouton marche, c'est le menu qui ne marche pas.",
                "Le bouton marche, c'est le menu qui ne marche pas.",
            ),
            ("I haven't sent it yet.", "I have not sent it yet."),
            // Field false alarms of the first draft, anonymised.
            (
                "Le serveur de test qui est pas pratique.",
                "Le serveur de test peu pratique.",
            ),
            (
                "Comme ça on n'a pas à le retaper dans le formulaire.",
                "Pour ne pas avoir à le retaper dans le formulaire.",
            ),
            (
                "Est-ce qu'on pourrait pas accélérer l'export parce qu'il est lent.",
                "On pourrait accélérer l'export, car il est lent.",
            ),
            (
                "Le vieux module, je le mets pas à jour.",
                "Le vieux module n'est plus mis à jour.",
            ),
        ] {
            assert_eq!(check(raw, cleaned), Ok(()), "{cleaned}");
        }
    }

    #[test]
    fn rex_nov_2026_a_pending_status_must_survive() {
        // f9d3ea95: the status vanished from a long rewrite that kept other negations.
        let raw = "Alors attends, j'ai pas encore envoyé le devis au client. Par contre, j'ai fait l'essai sur la vieille imprimante et elle sort des pages blanches. Je ne l'utilise plus.";
        let dropped = "J'ai fait l'essai sur la vieille imprimante et elle sort des pages blanches. Je ne l'utilise plus.";
        assert_eq!(check(raw, dropped), Err("pending_lost"));
        assert_eq!(
            explain(raw, dropped, "pending_lost").as_deref(),
            Some("état en attente disparu : « pas encore »")
        );
        for kept in [
            "Je n'ai pas encore envoyé le devis au client. J'ai fait l'essai sur la vieille imprimante et elle sort des pages blanches. Je ne l'utilise plus.",
            "Je n'ai toujours pas envoyé le devis au client. J'ai fait l'essai sur la vieille imprimante et elle sort des pages blanches. Je ne l'utilise plus.",
        ] {
            assert_eq!(check(raw, kept), Ok(()), "{kept}");
        }
        assert_eq!(
            check("We haven't shipped it yet.", "We shipped it."),
            Err("pending_lost")
        );
        assert_eq!(
            check(
                "We haven't shipped the fix yet. The build is not green.",
                "The build is not green."
            ),
            Err("pending_lost")
        );
    }

    #[test]
    fn rex_nov_2026_a_dropped_condition_is_rejected() {
        // 70355dc8: the « s'il n'y a pas de… » became a concession.
        let raw = "Je te confirme, avec la nouvelle version, l'export marche très bien. Enfin, je veux dire, il n'y a pas l'alerte rouge s'il n'y a pas d'erreur.";
        let lost = "Je te confirme que l'export de la nouvelle version marche très bien, bien qu'il n'y ait pas d'alerte rouge.";
        assert_eq!(check(raw, lost), Err("condition_lost"));
        assert_eq!(
            explain(raw, lost, "condition_lost").as_deref(),
            Some("condition disparue : « erreur »")
        );
        for kept in [
            "Je te confirme qu'avec la nouvelle version, l'export marche très bien : il n'y a pas l'alerte rouge s'il n'y a pas d'erreur.",
            "Je te confirme qu'avec la nouvelle version, l'export marche très bien. Sans erreur, l'alerte rouge n'apparaît pas.",
            "Je te confirme qu'avec la nouvelle version, l'export marche très bien. L'alerte rouge n'apparaît qu'en cas d'erreur.",
        ] {
            assert_eq!(check(raw, kept), Ok(()), "{kept}");
        }
        assert_eq!(
            check(
                "Envoie le rapport si les tests passent.",
                "Envoie le rapport, les tests passent."
            ),
            Err("condition_lost")
        );
        assert_eq!(
            check(
                "Deploy on Friday if the tests pass.",
                "Deploy on Friday, the tests pass."
            ),
            Err("condition_lost")
        );
        for (raw, cleaned) in [
            (
                "Euh, si tu as le temps, regarde le rapport.",
                "Si tu as le temps, regarde le rapport.",
            ),
            ("Si c'est bon, envoie-le.", "Envoie-le si c'est bon."),
            (
                "Si les tests passent, on déploie vendredi.",
                "On déploie vendredi quand les tests passent.",
            ),
            // « si » meaning whether, as if or even if is not a condition.
            (
                "Je sais pas si c'est bon.",
                "Je ne suis pas sûr que ce soit bon.",
            ),
            (
                "Même si c'est lent, ça marche.",
                "Bien que ce soit lent, ça marche.",
            ),
            ("I wonder if it works.", "I wonder whether it works."),
            (
                "La recherche marche pas avec les accents, genre si je tape « été » ça trouve rien.",
                "La recherche ne fonctionne pas avec les accents (par exemple, taper « été » ne donne rien).",
            ),
            (
                "Je veux voir si je relance le service tout de suite, si ça change quelque chose.",
                "Je veux voir si la demande de relancer le service tout de suite change quelque chose.",
            ),
            (
                "Regarde les tickets. S'il y a des corrections possibles à faire.",
                "Regarde les tickets et note d'éventuelles corrections.",
            ),
        ] {
            assert_eq!(check(raw, cleaned), Ok(()), "{cleaned}");
        }
    }

    #[test]
    fn rex_nov_2026_a_dropped_version_segment_is_rejected() {
        // 05ee43b7: 1.7.1 became 1.7 where 1.7 was also said.
        let raw = "On est passés de la un point sept à la un point sept point un hier soir.";
        assert_eq!(
            check(raw, "On est passés de la 1.7 à la 1.7 hier soir."),
            Err("number_lost")
        );
        assert_eq!(
            explain(
                raw,
                "On est passés de la 1.7 à la 1.7 hier soir.",
                "number_lost"
            )
            .as_deref(),
            Some("nombre du brut absent de la reformulation : « 1.7.1 »")
        );
        assert_eq!(
            check(raw, "On est passés de la 1.7 à la 1.7.1 hier soir."),
            Ok(())
        );
        for (raw, bad, reason) in [
            (
                "Installe la deux point quatre point un.",
                "Installe la 2.4.",
                "number_changed",
            ),
            (
                "Installe la deux point quatre point un.",
                "Installe la 2.4.0.",
                "number_changed",
            ),
            (
                "Installe la deux point quatre point un.",
                "Installe la 2.4.2.",
                "number_changed",
            ),
            (
                "Python trois point douze point zéro puis Python trois point douze.",
                "Python 3.12 puis Python 3.12.",
                "number_lost",
            ),
            (
                "L'IP 192.168.1.50 remplace la 192.168.1.",
                "L'IP 192.168.1 remplace la 192.168.1.",
                "number_lost",
            ),
        ] {
            assert_eq!(check(raw, bad), Err(reason), "{bad}");
        }
        for (raw, cleaned) in [
            (
                "Installe la deux point quatre point un.",
                "Installe la 2.4.1.",
            ),
            ("Python trois point douze point zéro.", "Python 3.12.0."),
            (
                "L'adresse est cent quatre-vingt-douze point cent soixante-huit point un point cinquante.",
                "L'adresse est 192.168.1.50.",
            ),
            (
                "Il n'y a qu'un point à discuter.",
                "Il n'y a qu'un point à discuter.",
            ),
            (
                "D'abord tu installes la un point sept point un, ensuite tu redémarres.",
                "1. Installe la 1.7.1.\n2. Redémarre.",
            ),
            (
                "Le taux est de trois point cinq pour cent.",
                "Le taux est de 3,5 %.",
            ),
            // A version taken back may go.
            (
                "Installe la un point sept point un, non, la un point sept point deux.",
                "Installe la 1.7.2.",
            ),
        ] {
            assert_eq!(check(raw, cleaned), Ok(()), "{cleaned}");
        }
    }

    #[test]
    fn rex_nov_2026_a_unit_is_checked_against_its_own_number() {
        // b5a55eaa: « minutes » said elsewhere hid the invented one.
        let raw = "Le build prend cinq minutes. On a peut-être gagné à peu près vingt de temps.";
        let invented = "Le build prend 5 minutes. On a peut-être gagné environ 20 minutes.";
        assert_eq!(check(raw, invented), Err("unit_added"));
        assert_eq!(
            explain(raw, invented, "unit_added").as_deref(),
            Some("unité absente du brut : « minutes »")
        );
        for (raw, cleaned) in [
            (
                "Le build prend cinq minutes. On a peut-être gagné à peu près vingt de temps.",
                "Le build prend 5 minutes. On a peut-être gagné à peu près vingt de temps.",
            ),
            (
                "Je viens à dix heures et je repars à onze heures.",
                "Je viens à 10 h et je repars à 11 h.",
            ),
            (
                "Il a seize gigas de RAM et deux gigas de swap.",
                "Il a 16 Go de RAM et 2 Go de swap.",
            ),
            (
                "Ça dure quelques minutes, pas plus.",
                "Ça dure quelques minutes, pas plus.",
            ),
            // The amount belongs to « euros », not to « mois ».
            (
                "Le loyer fait mille quatre cents euros par mois.",
                "Le loyer fait 1 400 € par mois.",
            ),
        ] {
            assert_eq!(check(raw, cleaned), Ok(()), "{cleaned}");
        }
    }

    #[test]
    fn rex_nov_2026_a_product_name_keeps_its_parts() {
        // f9d3ea95: « Notepad plus plus » lost a « plus » as a repetition.
        let raw = "La même phrase dans Notepad plus plus marche très bien.";
        assert_eq!(
            check(raw, "La même phrase dans Notepad plus marche très bien."),
            Err("name_changed")
        );
        for kept in [
            "La même phrase dans Notepad plus plus marche très bien.",
            "La même phrase dans Notepad++ marche très bien.",
        ] {
            assert_eq!(check(raw, kept), Ok(()), "{kept}");
        }
        assert_eq!(
            check(
                "De plus, plus personne ne répond.",
                "De plus, personne ne répond."
            ),
            Ok(())
        );
    }

    #[test]
    fn rex_nov_2026_field_successes_still_pass() {
        // 495d8029: the step order the speaker corrected.
        assert_eq!(
            check(
                "Pour monter l'étagère, tu fixes les montants, ensuite tu poses les planches et après tu serres les vis. Ah non, attends, avant de fixer les montants, il faut d'abord vider le carton et quand tout tient, tu ranges les outils.",
                "Pour monter l'étagère :\n1. Vide le carton.\n2. Fixe les montants.\n3. Pose les planches.\n4. Serre les vis.\n5. Range les outils."
            ),
            Ok(())
        );
        // d3187fa0: the last quantity wins, the cheese is dropped.
        let shopping = "Pour samedi, il faudrait acheter deux bouteilles d'eau, trois pommes, du fromage, une baguette et quatre yaourts. Ah non, finalement pas de fromage, j'en ai encore. Et pour les pommes, je me suis trompé, il en faut cinq, pas trois. Ajoute aussi du café.";
        for cleaned in [
            "Pour samedi :\n- Deux bouteilles d'eau.\n- Cinq pommes.\n- Une baguette.\n- Quatre yaourts.\n- Du café.",
            "Pour samedi :\n- Deux bouteilles d'eau.\n- Cinq pommes.\n- Une baguette.\n- Quatre yaourts.\n- Du café.\n\nPas de fromage, il en reste.",
        ] {
            assert_eq!(check(shopping, cleaned), Ok(()), "{cleaned}");
        }
        // « ce n'est pas mille deux cents, c'est mille quatre cents »: 1200 is taken back.
        assert_eq!(
            check(
                "Le loyer, ce n'est pas mille deux cents euros, c'est mille quatre cents euros par mois.",
                "Le loyer est de 1 400 euros par mois."
            ),
            Ok(())
        );
        // The retracted quantity must not come back as the last word.
        assert_eq!(
            check(
                shopping,
                "Pour samedi :\n- Deux bouteilles d'eau.\n- Trois pommes.\n- Une baguette.\n- Quatre yaourts.\n- Du café."
            ),
            Err("number_lost")
        );
        // 926c3791: a long monologue kept in paragraphs, hedges and negations included.
        assert_eq!(
            check(
                "bon alors la recette elle est plutôt réussie c'est pas très original c'est vrai que quand t'en as mangé deux trois parts t'as envie d'autre chose après peut-être qu'une version avec un piment deux piments ou trois piments ça peut apporter un plus mais là c'est encore trop fade et c'est pas assez croustillant et par contre ce qui est sympa c'est qu'on a jamais deux fois la même garniture parce que tu reprends pas toujours la même chose et si cette sauce elle est pas plus relevée ça sert à rien d'en refaire je trouve que ça cuit pas encore assez",
                "Bon, la recette est plutôt réussie. C'est pas très original. C'est vrai que quand t'en as mangé deux ou trois parts, t'as envie d'autre chose. Après, peut-être qu'une version avec un piment, deux piments ou trois piments, ça peut apporter un plus.\n\nMais là, c'est encore trop fade et c'est pas assez croustillant. Par contre, ce qui est sympa, c'est qu'on a jamais deux fois la même garniture, parce que tu reprends pas toujours la même chose.\n\nSi cette sauce n'est pas plus relevée, ça sert à rien d'en refaire. Je trouve que ça cuit pas encore assez."
            ),
            Ok(())
        );
    }

    #[test]
    fn legitimate_cleanups_pass() {
        // A spoken path: « deux-points » is punctuation (seen on the bench).
        assert_eq!(
            check(
                "Lance cargo test puis git status dans C deux-points backslash projet.",
                "Lance cargo test, puis git status dans C:\\projet."
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "Copie le fichier C deux points antislash temp antislash config point json.",
                "Copie le fichier C:\\temp\\config.json."
            ),
            Ok(())
        );
        // « deux points » may also be a real quantity in the output (bench h2-long-retour).
        let two = "Il y a deux trucs qui m'ont gêné, le bouton et la recherche.";
        assert_eq!(
            check(two, "Deux points m'ont gêné : le bouton et la recherche."),
            Ok(())
        );
        assert_eq!(
            check(two, "Le bouton et la recherche m'ont gêné."),
            Err("number_lost")
        );
        // Self-correction drops the abandoned number and the retracted item.
        assert_eq!(
            check(
                "Je viens à neuf heures. Enfin non, je viens à dix heures.",
                "Je viens à 10 h."
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "Il faut des chips, du fromage, ah non pas de fromage, et des serviettes.",
                "Il faut :\n- des chips ;\n- des serviettes."
            ),
            Ok(())
        );
        // Numbered steps add list markers the speaker never said.
        assert_eq!(
            check(
                "D'abord tu télécharges l'image, ensuite tu la copies, et enfin tu redémarres.",
                "1. Télécharge l'image.\n2. Copie-la.\n3. Redémarre."
            ),
            Ok(())
        );
        // Articles and spoken numbers rewritten as digits.
        assert_eq!(
            check(
                "Il me faut quatre gigas et un câble.",
                "Il me faut 4 Go et le câble."
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "Je pense que c'est bon, enfin je crois.",
                "Je pense que c'est bon."
            ),
            Ok(())
        );
    }
}

#[cfg(test)]
mod bench_references {
    use super::check;

    /// Every hand-written reference of the bench corpora is a faithful rewrite:
    /// the gate must let them through (false-positive guard).
    #[test]
    fn bench_references_pass_the_gate() {
        let corpora = [
            include_str!("../../../docs/v15/bench/reformulation_cases.json"),
            include_str!("../../../docs/v15/bench/reformulation_holdout.json"),
            include_str!("../../../docs/v15/bench/reformulation_holdout2.json"),
            include_str!("../../../docs/v15/bench/smart_control.json"),
            include_str!("../../../docs/v15/bench/smart_dev.json"),
            include_str!("../../../docs/v151/bench/fidelity_cases.json"),
            include_str!("../../../docs/v171/bench/number_cases.json"),
        ];
        let mut refused = Vec::new();
        let mut total = 0;
        for corpus in corpora {
            let cases: Vec<serde_json::Value> = serde_json::from_str(corpus).unwrap();
            for case in &cases {
                let (Some(raw), Some(reference)) =
                    (case["input"].as_str(), case["reference"].as_str())
                else {
                    continue;
                };
                total += 1;
                if let Err(reason) = check(raw, reference) {
                    refused.push(format!("{} {reason}", case["id"]));
                }
            }
        }
        assert_eq!(total, 125);
        // Known strictness since numbers are only excused near a correction:
        // c-long-450 is a bullet summary that drops « deuxième/troisième sujet »,
        // d-long-retour-produit drops the closing recap « deux ou trois soucis ».
        // A rewrite like either falls back to the raw text, which stays faithful.
        assert_eq!(
            refused,
            [
                "\"c-long-450\" number_lost",
                "\"d-long-retour-produit\" number_lost"
            ]
        );
    }
}
