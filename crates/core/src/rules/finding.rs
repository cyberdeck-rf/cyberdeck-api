//! Type [`Finding`] — résultat de l'évaluation d'une règle qui a matché.
//!
//! Côté Dart le miroir est `AppFinding` (cf. `mobile-app/rust/src/api/types.rs`).
//! Le `Finding` est produit par le [`crate::rules::RulesEngine`] et émis sur
//! un `broadcast::Sender<AppFinding>` dans le ClientTask de la mobile-app
//! (cf. `mobile-app/rust/src/api/client.rs`).

use crate::proto::schema::Module;
use crate::rules::schema::Severity;
use serde::{Deserialize, Serialize};

/// Une vulnérabilité détectée — la conjonction d'une règle et de la frame
/// qui l'a déclenchée.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// ID de la règle source (`SUBGHZ-001`, `NFC-002`, …).
    pub rule_id: String,
    /// Titre de la règle (copie pour autonomie côté UI).
    pub title: String,
    /// Sévérité de la règle (copie).
    pub severity: Severity,
    /// Module sur lequel la frame a été reçue.
    pub module: Module,
    /// Fréquence (Hz) de la frame.
    pub freq_hz: u32,
    /// Payload (hex majuscule) ayant déclenché le finding.
    pub payload_hex: String,
    /// Timestamp millisecondes (recopié depuis la frame).
    pub ts_ms: u64,
    /// Description longue (copie depuis la règle).
    pub description: String,
    /// Recommandation de mitigation (copie depuis la règle).
    pub recommendation: String,
    /// Références bibliographiques (CVE / paper / cheatsheet).
    pub references: Vec<String>,
}
