//! Fenêtre glissante et évaluation des opérateurs stateful.
//!
//! On garde un buffer FIFO `VecDeque<RfFrame>` par module, plafonné en
//! taille (`capacity_per_module`).  Les opérations stateful inspectent ce
//! buffer pour détecter :
//!
//! * un payload immobile sur N frames consécutives (rolling code KO) ;
//! * un replay (payload déjà vu il y a moins de `ms` millisecondes) ;
//! * un burst d'émissions sur la même fréquence dans une fenêtre temporelle.
//!
//! Le buffer est inséré PUIS évalué (la frame courante fait partie de la
//! fenêtre observable).  Cf. `engine.rs` pour l'ordre exact d'opération.

use crate::proto::schema::{Module, RfFrame};
use crate::rules::schema::StatefulOp;
use std::collections::{HashMap, VecDeque};

/// Capacité par défaut du buffer glissant (par module).
/// 64 frames = ~ 1 min de capture à 1 Hz, suffisant pour les règles de
/// replay et de burst-count typiques.
pub const DEFAULT_CAPACITY: usize = 64;

/// Buffer glissant par module — FIFO plafonné.
#[derive(Debug, Clone)]
pub struct RollingBuffer {
    /// File par module. Une `VecDeque` permet du O(1) push_back / pop_front.
    pub per_module: HashMap<Module, VecDeque<RfFrame>>,
    /// Plafond par module — au-delà on jette la plus ancienne.
    pub capacity_per_module: usize,
}

impl RollingBuffer {
    /// Construit un buffer vide à capacité `cap` par module.
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self { per_module: HashMap::new(), capacity_per_module: cap.max(1) }
    }

    /// Construit un buffer vide à capacité par défaut.
    #[must_use]
    pub fn default_cap() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Pousse une frame dans le buffer du bon module, en respectant la
    /// capacité (drop oldest).  Renvoie une référence à la frame insérée
    /// (utile pour chaining).
    pub fn push(&mut self, frame: RfFrame) {
        let cap = self.capacity_per_module;
        let dq  = self.per_module.entry(frame.module).or_default();
        if dq.len() >= cap {
            dq.pop_front();
        }
        dq.push_back(frame);
    }

    /// Itère la fenêtre d'un module dans l'ordre d'arrivée (oldest first).
    pub fn frames_for(&self, module: Module) -> impl Iterator<Item = &RfFrame> {
        self.per_module.get(&module).into_iter().flat_map(|d| d.iter())
    }
}

/// Évalue un opérateur stateful sur le buffer courant.
///
/// La frame qui déclenche l'évaluation doit DÉJÀ avoir été poussée dans
/// le buffer (cf. `engine.rs`) — c'est cohérent avec la sémantique « la N-ième
/// frame déclenche le finding ».
///
/// `window` est la valeur de `rule.window_frames` ; les opérateurs qui n'en
/// ont pas besoin (`PayloadReplayWithinMs`, `FreqBurstCount`) l'ignorent.
#[must_use]
pub fn evaluate_stateful(
    op: &StatefulOp,
    module: Module,
    window: u32,
    buffer: &RollingBuffer,
) -> bool {
    let Some(dq) = buffer.per_module.get(&module) else { return false; };

    match op {
        StatefulOp::PayloadIdenticalInWindow => {
            // Window de N : on regarde les N dernières frames du module, et
            // on vérifie qu'elles partagent toutes le même payload_hex.
            // Si window == 0 on considère que la règle ne match pas (garde-fou).
            if window == 0 { return false; }
            let n = window as usize;
            if dq.len() < n { return false; }
            // Slice des N derniers.
            let tail: Vec<&RfFrame> = dq.iter().rev().take(n).collect();
            let first_hex = &tail[0].payload_hex;
            tail.iter().all(|f| &f.payload_hex == first_hex)
        }
        StatefulOp::PayloadReplayWithinMs(ms) => {
            // La dernière frame poussée est la "courante".  On cherche dans
            // l'historique (toutes sauf la dernière) un payload identique
            // avec `ts_now - ts_seen < ms`.
            let Some(current) = dq.back() else { return false; };
            let ts_now = current.ts_ms;
            let hex_now = &current.payload_hex;
            // skip(last 1) en itérant à l'envers → on saute la frame courante.
            dq.iter().rev().skip(1).any(|f| {
                &f.payload_hex == hex_now
                    && ts_now.saturating_sub(f.ts_ms) < *ms
            })
        }
        StatefulOp::FreqBurstCount { count, within_ms } => {
            // Compte les frames partageant la même freq que la courante
            // dans la fenêtre temporelle [ts_now - within_ms, ts_now].
            let Some(current) = dq.back() else { return false; };
            let ts_now = current.ts_ms;
            let target_freq = current.freq;
            let lo = ts_now.saturating_sub(*within_ms);
            let n = dq.iter().filter(|f| {
                f.freq == target_freq && f.ts_ms >= lo && f.ts_ms <= ts_now
            }).count() as u32;
            n >= *count
        }
    }
}
