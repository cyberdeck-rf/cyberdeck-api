//! Moteur principal — [`RulesEngine`] : agrège pack TOML + buffer glissant
//! + dispatch des matchers stateless/stateful.
//!
//! # Concurrence
//!
//! * `pack` est sous `RwLock` — lectures concurrentes, écriture exclusive
//!   uniquement quand l'updater HTTPS (M5) remplace le pack à chaud.
//! * `buffer` est sous `Mutex` — `evaluate` est appelée séquentiellement
//!   depuis le ClientTask single-owner (cf. `mobile-app/rust/src/api/client.rs`),
//!   donc la contention est nulle en pratique.  Un `Mutex` suffit largement.
//!
//! # Référentiel de fréquences attendu
//!
//! Les bandes utilisées dans le pack par défaut :
//!
//! | Band            | Module       | Range                    |
//! |-----------------|--------------|--------------------------|
//! | RFID LF 125 kHz | `rfid_lf`    | 120_000 – 135_000 Hz     |
//! | Sub-GHz 433/868 | `sx1262`     | 430 – 868 MHz            |
//! | NFC 13.56 MHz   | `st25r3916`  | 13_500_000 – 13_600_000  |
//! | WiFi/BLE 2.4 G  | `esp32c6`    | 2_400 – 2_500 MHz        |
//! | LoRa 2.4 GHz    | `sx1280`     | 2_400 – 2_500 MHz        |

use crate::proto::schema::RfFrame;
use crate::rules::finding::Finding;
use crate::rules::matcher::evaluate_stateless;
use crate::rules::schema::{MatchSpec, RulePack};
use crate::rules::stateful::{evaluate_stateful, RollingBuffer};
use std::sync::{Mutex, RwLock};

/// Moteur d'évaluation des règles.  Owne le pack courant et le buffer
/// glissant ; expose `evaluate(&frame) -> Vec<Finding>`.
pub struct RulesEngine {
    /// Pack courant. Mutable à chaud via [`Self::replace_pack`] (utilisé par
    /// l'updater HTTPS distant — autre agent, M5).
    pack: RwLock<RulePack>,
    /// Buffer glissant par module — alimente les opérateurs stateful.
    buffer: Mutex<RollingBuffer>,
}

impl RulesEngine {
    /// Charge le pack par défaut embarqué dans le binaire via `include_str!`.
    /// C'est le FALLBACK utilisé quand le pack distant n'est pas joignable.
    pub fn from_default() -> Result<Self, String> {
        let toml_src = include_str!("catalog/default_rules.toml");
        let pack = RulePack::from_toml_str(toml_src)?;
        Ok(Self::with_pack(pack))
    }

    /// Construit un engine avec un pack arbitraire (utile pour tests / remote).
    #[must_use]
    pub fn with_pack(pack: RulePack) -> Self {
        Self {
            pack:   RwLock::new(pack),
            buffer: Mutex::new(RollingBuffer::default_cap()),
        }
    }

    /// Remplace atomiquement le pack courant. Le buffer glissant est conservé
    /// — les findings émis juste après le swap peuvent donc déjà bénéficier
    /// d'un historique côté stateful (sémantique voulue).
    pub fn replace_pack(&self, new: RulePack) {
        if let Ok(mut guard) = self.pack.write() {
            *guard = new;
        }
    }

    /// Renvoie `(pack_version, rules_count)`.
    #[must_use]
    pub fn pack_info(&self) -> (String, usize) {
        match self.pack.read() {
            Ok(p)  => (p.pack_version.clone(), p.rules.len()),
            Err(_) => (String::from("?"), 0),
        }
    }

    /// Cœur du moteur — évalue toutes les règles pour cette frame.
    ///
    /// Algorithme :
    ///   1. Push la frame dans le buffer glissant (mod par mod).
    ///   2. Pour chaque règle ciblant le module de la frame, applique le
    ///      `MatchSpec` (stateless ALL/ANY ou stateful).
    ///   3. Construit un [`Finding`] par règle qui match.
    pub fn evaluate(&self, frame: &RfFrame) -> Vec<Finding> {
        // Buffer first : ainsi les opérateurs stateful « voient » la frame
        // courante (cohérent avec « la N-ième frame déclenche »).
        {
            let mut buf = match self.buffer.lock() {
                Ok(b)  => b,
                Err(p) => p.into_inner(), // poison : on récupère quand même
            };
            buf.push(frame.clone());
        }

        let pack = match self.pack.read() {
            Ok(p)  => p,
            Err(p) => p.into_inner(),
        };

        let module_str = frame.module.as_protocol_str();
        let mut findings = Vec::new();

        for rule in &pack.rules {
            // Filtre amont par module.
            if !rule.modules.iter().any(|m| m == module_str) {
                continue;
            }

            let matched = match &rule.r#match {
                MatchSpec::All(list) => {
                    !list.is_empty() && list.iter().all(|m| evaluate_stateless(m, frame))
                }
                MatchSpec::Any(list) => {
                    list.iter().any(|m| evaluate_stateless(m, frame))
                }
                MatchSpec::Stateful(op) => {
                    let win = rule.window_frames.unwrap_or(0);
                    // Re-acquire buffer pour la lecture (le push a déjà eu
                    // lieu, mais on a relâché le mutex pour ne pas tenir
                    // deux locks en même temps).
                    let buf = match self.buffer.lock() {
                        Ok(b)  => b,
                        Err(p) => p.into_inner(),
                    };
                    evaluate_stateful(op, frame.module, win, &buf)
                }
            };

            if matched {
                findings.push(Finding {
                    rule_id:        rule.id.clone(),
                    title:          rule.title.clone(),
                    severity:       rule.severity,
                    module:         frame.module,
                    freq_hz:        frame.freq,
                    payload_hex:    frame.payload_hex.clone(),
                    ts_ms:          frame.ts_ms,
                    description:    rule.description.clone(),
                    recommendation: rule.recommendation.clone(),
                    references:     rule.references.clone(),
                });
            }
        }
        findings
    }
}
