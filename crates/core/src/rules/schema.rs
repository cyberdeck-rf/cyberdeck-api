//! Schéma TOML des règles pentest (sérialisé via serde).
//!
//! Le format est documenté dans `docs/rules-schema.md` (et en commentaire en
//! tête de `catalog/default_rules.toml`).  La grammaire TOML est :
//!
//! ```toml
//! schema_version = 1
//! pack_version   = "2026.05.21"
//! source         = "default"
//!
//! [[rule]]
//! id             = "RFID-LF-001"
//! title          = "..."
//! severity       = "high"
//! modules        = ["rfid_lf"]
//! description    = "..."
//! recommendation = "..."
//! references     = ["..."]
//! window_frames  = 3        # optionnel
//!
//! [rule.match]
//! # exactement UN parmi : `all`, `any`, `stateful`
//! all = [
//!   { freq_range = [120_000, 135_000] },
//!   { payload_length_between = [5, 8] },
//!   { payload_hex_starts_with = "04" },
//!   { payload_hex_matches = "^[0-9A-F]+$" },
//! ]
//! ```
//!
//! Le mapping TOML → enum se fait avec `#[serde(untagged)]` pour les
//! variantes de matchers (TOML utilise un dict à clef unique pour discriminer
//! chaque matcher stateless) — c'est le pattern qui colle le mieux à la
//! grammaire `{ freq_range = [...] }` côté TOML.

use serde::{Deserialize, Serialize};

// =============================================================================
// Severity
// =============================================================================

/// Niveau de gravité d'une règle (et donc d'un finding).
///
/// Ordre croissant (info < critical) utile pour trier l'UI :
/// cf. [`Severity::order`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Information purement contextuelle.
    Info,
    /// Indice faible — à vérifier mais non bloquant.
    Low,
    /// Vulnérabilité réelle mais à impact limité.
    Medium,
    /// Vulnérabilité exploitable, à corriger rapidement.
    High,
    /// Vulnérabilité critique, exploitation triviale, action immédiate.
    Critical,
}

impl Severity {
    /// String courte stable (ASCII lowercase) — utile pour les logs / UI.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info     => "info",
            Severity::Low      => "low",
            Severity::Medium   => "medium",
            Severity::High     => "high",
            Severity::Critical => "critical",
        }
    }

    /// Rang numérique croissant (utile pour tri / filtrage UI).
    #[must_use]
    pub fn order(&self) -> u8 {
        match self {
            Severity::Info     => 0,
            Severity::Low      => 1,
            Severity::Medium   => 2,
            Severity::High     => 3,
            Severity::Critical => 4,
        }
    }
}

// =============================================================================
// Matchers stateless
// =============================================================================

/// Un matcher stateless qui s'évalue sur UNE frame.
///
/// La discriminante TOML est portée par la clef unique du dict
/// (`{ freq_range = [...] }`) — `#[serde(untagged)]` essaie chaque variante
/// dans l'ordre, ce qui marche tant que chaque variante a un nom de champ
/// unique.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Matcher {
    /// `freq` ∈ [min, max] (Hz, inclus).
    FreqRange {
        /// Bornes inclusives `[min, max]` en Hz.
        freq_range: [u32; 2],
    },
    /// Longueur du payload en BYTES (`payload_hex.len()/2`) ∈ [min, max].
    PayloadLengthBetween {
        /// Bornes inclusives `[min, max]` en octets.
        payload_length_between: [u32; 2],
    },
    /// Le payload (hex majuscule) commence par ce préfixe.
    PayloadHexStartsWith {
        /// Préfixe attendu (hex majuscule, sans `0x`).
        payload_hex_starts_with: String,
    },
    /// Le payload matche cette regex (cache OnceLock dans `matcher.rs`).
    PayloadHexMatches {
        /// Regex Rust standard (crate `regex`).
        payload_hex_matches: String,
    },
}

// =============================================================================
// Opérateurs stateful
// =============================================================================

/// Opérateur stateful — opère sur la fenêtre glissante par module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulOp {
    /// Les N dernières frames (= `window_frames` de la règle) ont toutes
    /// le même `payload_hex`.  Détecte les rolling codes immobiles.
    PayloadIdenticalInWindow,
    /// La frame courante a un `payload_hex` déjà vu dans le buffer,
    /// avec `(ts_now - ts_seen) < ms`.  Détecte les replays.
    PayloadReplayWithinMs(u64),
    /// `count` frames ou plus avec la même `freq_hz` dans la fenêtre
    /// temporelle `within_ms`.  Détecte les bursts (jamming / flood).
    FreqBurstCount {
        /// Seuil de comptage.
        count: u32,
        /// Fenêtre temporelle en millisecondes.
        within_ms: u64,
    },
}

// =============================================================================
// Spec de match
// =============================================================================

/// Spécification de match : `all` (ET logique), `any` (OU logique), ou
/// `stateful`.  Exactement une des trois clefs est présente dans le TOML.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchSpec {
    /// ET logique — TOUS les matchers doivent passer.
    #[serde(rename = "all")]
    All(Vec<Matcher>),
    /// OU logique — au moins UN matcher doit passer.
    #[serde(rename = "any")]
    Any(Vec<Matcher>),
    /// Stateful — un seul opérateur, évalué sur la fenêtre glissante.
    #[serde(rename = "stateful")]
    Stateful(StatefulOp),
}

// =============================================================================
// Rule + RulePack
// =============================================================================

/// Une règle pentest individuelle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// ID stable au format `<BAND>-<NNN>` (cf. catalogue).
    pub id: String,
    /// Titre affiché à l'utilisateur.
    pub title: String,
    /// Sévérité (`info` … `critical`).
    pub severity: Severity,
    /// Modules concernés (au moins un — filtre amont avant matching).
    pub modules: Vec<String>,
    /// Description longue (français — c'est ce que l'utilisateur lit).
    pub description: String,
    /// Recommandation de mitigation (français).
    pub recommendation: String,
    /// CVE / RFC / papers / cheatsheet — pour la défense en audit.
    #[serde(default)]
    pub references: Vec<String>,
    /// Taille de la fenêtre stateful (frames). `None` si stateless.
    #[serde(default)]
    pub window_frames: Option<u32>,
    /// Spec de match (cf. [`MatchSpec`]).
    #[serde(rename = "match")]
    pub r#match: MatchSpec,
}

/// Un pack complet de règles (ce qui est chargé en mémoire).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulePack {
    /// Version du schéma (entier, bump si breaking).
    pub schema_version: u32,
    /// Version du pack (semver / date — affiché dans l'UI).
    pub pack_version: String,
    /// Origine du pack (`"default"`, `"remote"`, …).
    pub source: String,
    /// Liste des règles.
    #[serde(rename = "rule", default)]
    pub rules: Vec<Rule>,
}

impl RulePack {
    /// Parse un pack TOML depuis une string. Renvoie une string d'erreur
    /// humainement lisible (suffisant — on n'utilise pas thiserror ici car
    /// les erreurs remontent toutes au point d'init).
    pub fn from_toml_str(s: &str) -> Result<Self, String> {
        toml::from_str(s).map_err(|e| format!("rules pack TOML parse error: {e}"))
    }
}
