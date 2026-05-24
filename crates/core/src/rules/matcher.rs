//! Évaluation des matchers stateless.
//!
//! Les regex sont compilées paresseusement et **cachées** dans un
//! `OnceLock<Mutex<HashMap<…>>>` au niveau module — on évite ainsi le coût
//! de recompilation à chaque frame (les regex de la cheatsheet typique
//! comme `^[A-F0-9]+$` ne sont pas gratuites).
//!
//! La cache est globale (pas par engine) car les regex sont déterministes :
//! deux engines qui voient la même regex partagent l'objet compilé.

use crate::proto::schema::RfFrame;
use crate::rules::schema::Matcher;
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Cache globale des regex compilées. Clef = source textuelle, valeur =
/// regex compilée OU `None` si la regex est invalide (on cache aussi les
/// échecs pour ne pas re-tenter à chaque frame).
fn regex_cache() -> &'static Mutex<HashMap<String, Option<Regex>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Regex>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lookup-or-compile une regex avec mémoïsation. `None` = pattern invalide
/// (déjà testé, ne retentera pas).
fn cached_regex(pattern: &str) -> Option<Regex> {
    // Fast-path : lecture seule.
    if let Ok(map) = regex_cache().lock() {
        if let Some(entry) = map.get(pattern) {
            return entry.clone();
        }
    }
    // Slow-path : compile puis insert. On accepte une double-compile dans le
    // cas pathologique de race — c'est strictement idempotent.
    let compiled = Regex::new(pattern).ok();
    if let Ok(mut map) = regex_cache().lock() {
        map.insert(pattern.to_string(), compiled.clone());
    }
    compiled
}

/// Évalue un matcher stateless sur une frame unique.
///
/// Renvoie `true` si la frame satisfait le matcher.  Sémantique exacte :
/// * `FreqRange [min, max]`           : `min <= frame.freq <= max`.
/// * `PayloadLengthBetween [min, max]`: `min <= (payload_hex.len()/2) <= max`.
/// * `PayloadHexStartsWith p`         : `frame.payload_hex.starts_with(p)`.
/// * `PayloadHexMatches r`            : `Regex(r).is_match(frame.payload_hex)`.
///
/// Une regex invalide est traitée comme « ne matche jamais » (cohérent avec
/// le pattern « pack distant un peu cassé ne doit pas crasher l'app »).
#[must_use]
pub fn evaluate_stateless(matcher: &Matcher, frame: &RfFrame) -> bool {
    match matcher {
        Matcher::FreqRange { freq_range: [min, max] } => {
            frame.freq >= *min && frame.freq <= *max
        }
        Matcher::PayloadLengthBetween { payload_length_between: [min, max] } => {
            // Longueur en BYTES (pas en chars hex).  Spec : "longueur en
            // BYTES, pas en caractères hex" — cf. spec dans l'énoncé.
            let n_bytes = (frame.payload_hex.len() / 2) as u32;
            n_bytes >= *min && n_bytes <= *max
        }
        Matcher::PayloadHexStartsWith { payload_hex_starts_with } => {
            frame.payload_hex.starts_with(payload_hex_starts_with.as_str())
        }
        Matcher::PayloadHexMatches { payload_hex_matches } => {
            match cached_regex(payload_hex_matches) {
                Some(re) => re.is_match(&frame.payload_hex),
                None     => false, // regex invalide → no match (cf. doc)
            }
        }
    }
}
