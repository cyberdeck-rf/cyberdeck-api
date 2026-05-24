// =============================================================================
//  cyberdeck-emu — émulateur firmware sur TCP
// =============================================================================
//
//  Émule le comportement d'un firmware cyberdeck branché en USB-CDC, mais
//  sur TCP.  Permet de développer la mobile-app sans matériel physique.
//
//  Protocole : strictement identique au firmware (cf. cyberdeck/src/proto/
//  schema.hpp + dispatcher.cpp).  Chaque connexion TCP donne lieu à une
//  session indépendante avec ses propres rings RX, son listening state,
//  son uptime virtuel.
//
//  Différences avec le vrai firmware :
//    * L'uptime démarre à 0 à chaque connexion (vs. boot MCU).
//    * Un auto-injector intégré ajoute des frames fake périodiquement sur
//      les modules en écoute, pour donner une UI vivante en démo.
//    * Pas de notion de débordement de ring : on garde le dernier message.
//
//  Usage : `cyberdeck-emu --listen 0.0.0.0:17017`.
//  En Docker : `docker compose up -d` lance ce binaire automatiquement.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use cyberdeck_api::proto::{codec, request, schema as s};
use cyberdeck_api::{
    Ack, Capabilities, Module, ModuleInfo, Nack, RfFrame, CAP_EMU, CAP_RX, CAP_SNIFF, CAP_TX,
};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{Duration, interval};

// =============================================================================
//  CLI
// =============================================================================
#[derive(Parser, Debug)]
#[command(
    name = "cyberdeck-emu",
    about = "TCP emulator of the RF Cyberdeck firmware",
    version,
)]
struct Args {
    /// Adresse d'écoute, host:port.  Par défaut `0.0.0.0:17017` (toutes les
    /// interfaces, port choisi pour ne pas collisionner avec un service connu).
    #[arg(long, default_value = "0.0.0.0:17017")]
    listen: String,

    /// Intervalle entre deux auto-injects en millisecondes (sur chaque
    /// module en écoute).  `0` désactive l'auto-injecteur — l'UI ne verra
    /// alors que les FRAMEs déclenchées par des INJECT explicites.
    #[arg(long, default_value_t = 1000)]
    auto_inject_ms: u64,
}

// =============================================================================
//  Identité statique reportée dans CAPABILITIES
// =============================================================================
const FW_VERSION: &str = "0.1.0-emu";
const HW_NAME:    &str = "cyberdeck-emu";

fn capabilities_for(module: Module) -> u32 {
    match module {
        Module::Sx1262    => CAP_RX | CAP_TX | CAP_SNIFF,
        Module::St25r3916 => CAP_RX | CAP_TX | CAP_SNIFF | CAP_EMU,
        Module::Esp32C6   => CAP_RX | CAP_TX | CAP_SNIFF,
        Module::Sx1280    => CAP_RX | CAP_TX,
        Module::RfidLf    => CAP_RX | CAP_TX | CAP_EMU,
    }
}

// =============================================================================
//  État d'une session
// =============================================================================
struct Session {
    started: Instant,
    listening: [bool; 5],
    freqs:     [u32;  5],
    rings:     [VecDeque<RfFrame>; 5],
}

impl Session {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            listening: [false; 5],
            freqs:     [0; 5],
            rings:     std::array::from_fn(|_| VecDeque::with_capacity(16)),
        }
    }

    fn uptime_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

// =============================================================================
//  Génération d'auto-injects (frames fake périodiques)
// =============================================================================
//
// Stratégie : cycler à travers un set de scénarios scriptés par module, choisi
// en fonction du `tick` global. Chaque scénario produit un payload calibré
// pour déclencher une règle PRÉCISE du catalogue `default_rules.toml`.
//
// L'objectif est qu'un listen sur l'ensemble des modules fasse déclencher au
// moins 8 règles distinctes en moins d'une minute (un cycle = 700 ms × tick).
//
// Référence : cyberdeck-api/crates/core/src/rules/catalog/default_rules.toml.

/// Choisit un scénario réaliste pour `module` à l'instant `tick` et retourne
/// `(freq_override, rssi, snr, payload_hex)`. La fréquence retournée peut
/// remplacer celle annoncée par `START_LISTEN` pour rester dans la bande
/// attendue par la règle (e.g. 125 kHz pour RFID-LF-001, 13.56 MHz pour NFC).
fn scenario_for(module: Module, tick: u64) -> (u32, i16, i8, String) {
    // RSSI / SNR pseudo-aléatoires mais déterministes — purement cosmétiques.
    let rssi = -45 - ((tick % 40) as i16);
    let snr = ((tick % 11) as i8) - 5;

    match module {
        // ---------------------------------------------------------------
        // SX1262 — sub-GHz. Cycle 3 scénarios sur fenêtres de 3 ticks :
        //   group 0 : payload IDENTIQUE 3 ticks d'affilée   → SUBGHZ-001
        //   group 1 : RKE 433 MHz court (4-16 bytes)        → SUBGHZ-002
        //   group 2 : trafic LoRa "normal" (ne déclenche rien de critique)
        // Cycle complet = 9 ticks.
        // ---------------------------------------------------------------
        Module::Sx1262 => {
            let group = (tick / 3) % 3;
            match group {
                // Rolling code fixé sur la durée du group (3 ticks IDENTIQUES).
                // L'index `tick / 3` change le payload entre cycles successifs
                // — mais reste constant pour 3 ticks consécutifs.
                0 => {
                    let id = (tick / 3) & 0xFFFF;
                    (433_920_000, rssi, snr, format!("DEADBEEFCAFE{id:04X}"))
                }
                // RKE OOK fixe — 6 octets dans la bande [430M, 435M].
                1 => {
                    let bits = (tick.wrapping_mul(0xA5A5)) & 0xFFFF_FFFF;
                    (433_420_000, rssi, snr, format!("A1B2{bits:08X}"))
                }
                // Trafic LoRa P2P "propre" — 868 MHz, payload long (ne match
                // ni SUBGHZ-001 ni SUBGHZ-002 ni SUBGHZ-003).
                _ => (
                    868_100_000,
                    rssi,
                    snr,
                    format!("4001{:06X}{:08X}AB", tick & 0xFFFFFF, tick),
                ),
            }
        }

        // ---------------------------------------------------------------
        // ST25R3916 — NFC 13.56 MHz. Cycle 4 scénarios :
        //   0 : UID Mifare Classic 4 octets `04XXXXXX` → NFC-001
        //   1 : trame avec clé default `FFFFFFFFFFFF`  → NFC-002
        //   2 : REQA `26…`                              → NFC-003
        //   3 : UID 7 octets `04…`                      → NFC-001 variant
        // ---------------------------------------------------------------
        Module::St25r3916 => {
            // On force la fréquence dans [13.5M, 13.6M] pour les `freq_range`.
            let freq = 13_560_000;
            let kind = tick % 4;
            match kind {
                0 => {
                    let uid = (tick.wrapping_mul(0xDEAD)) & 0xFF_FFFF;
                    (freq, rssi, snr, format!("04{uid:06X}"))
                }
                1 => {
                    // Préfixe d'auth Mifare + clé default 6 octets.
                    let sector = (tick & 0xFF) as u8;
                    (
                        freq,
                        rssi,
                        snr,
                        format!("60{sector:02X}FFFFFFFFFFFF"),
                    )
                }
                2 => (freq, rssi, snr, "2600".to_string()),
                _ => {
                    let uid = tick.wrapping_mul(0xC0FFEE);
                    (freq, rssi, snr, format!("04{:012X}", uid & 0xFFFF_FFFF_FFFF))
                }
            }
        }

        // ---------------------------------------------------------------
        // ESP32-C6 — WiFi 2.4 GHz. Cycle 3 scénarios :
        //   0 : beacon Open (frame ctrl `8000`)  → WIFI-001
        //   1 : EAPoL 4-way handshake (`888E`)   → WIFI-002
        //   2 : SMP pairing BLE (`1B`)           → BLE-001
        // ---------------------------------------------------------------
        Module::Esp32C6 => {
            // Force 2.4 GHz si la freq annoncée est ailleurs.
            let freq = 2_437_000_000;
            let kind = tick % 3;
            match kind {
                0 => {
                    // Beacon : frame control 8000 + duration + addr1 broadcast
                    // + un BSSID/SSID variable.
                    let bssid = tick.wrapping_mul(0x1337);
                    (
                        freq,
                        rssi,
                        snr,
                        format!("8000000FFFFFFFFFFFFF{:012X}", bssid & 0xFFFF_FFFF_FFFF),
                    )
                }
                1 => {
                    // EAPoL-Key (msg 1/4 ou 2/4) — ethertype 888E + version 02.
                    let nonce = tick.wrapping_mul(0xBADC0DE);
                    (
                        freq,
                        rssi,
                        snr,
                        format!("888E02030075{:016X}", nonce as u64),
                    )
                }
                _ => {
                    // SMP pairing request (BLE) — `1B` + IO caps + auth flags.
                    let v = (tick & 0xFF) as u8;
                    (freq, rssi, snr, format!("1B03000{v:02X}1010"))
                }
            }
        }

        // ---------------------------------------------------------------
        // SX1280 — LoRa 2.4 GHz. Cycle 3 scénarios :
        //   0 : ELRS control (`CC…`)             → LORA-001
        //   1 : LoRaWAN ADR MAC cmd (`03…`)      → LORA-002
        //   2 : beacon LoRa 2.4 long (8-64 octets) → LORA-003
        // ---------------------------------------------------------------
        Module::Sx1280 => {
            let freq = 2_400_000_000;
            let kind = tick % 3;
            match kind {
                0 => {
                    let chan = ((tick.wrapping_mul(0xE7B5)) & 0xFF_FFFF) as u32;
                    (freq, rssi, snr, format!("CC{chan:06X}"))
                }
                1 => {
                    let mac = tick.wrapping_mul(0xADAD) & 0xFFFF_FFFF;
                    (freq, rssi, snr, format!("03{mac:08X}"))
                }
                _ => {
                    let body = tick.wrapping_mul(0xBEEF);
                    (
                        freq,
                        rssi,
                        snr,
                        format!("4040{:016X}{:016X}", body, body.wrapping_add(1)),
                    )
                }
            }
        }

        // ---------------------------------------------------------------
        // RFID-LF — 125 kHz. Cycle 2 scénarios :
        //   0 : EM4102 5 octets    → RFID-LF-001
        //   1 : Indala (`A0…`)     → RFID-LF-003
        // (HID Prox 26-bit / 4 octets = RFID-LF-002, couvert par la fenêtre
        //  [5,8] de RFID-LF-001 sans collision puisque LF-002 demande length=4
        //  qu'on n'émet pas — c'est intentionnel pour éviter une matrice
        //  ambiguë côté moteur.)
        // ---------------------------------------------------------------
        Module::RfidLf => {
            let freq = 125_000;
            let kind = tick % 2;
            match kind {
                0 => {
                    let id = tick.wrapping_mul(0x4102) & 0xFFFF_FFFF_FF;
                    (freq, rssi, snr, format!("{id:010X}"))
                }
                _ => {
                    let body = tick.wrapping_mul(0x1DA1A) & 0xFFFF_FFFF;
                    (freq, rssi, snr, format!("A0{body:08X}"))
                }
            }
        }
    }
}

/// Construit la frame fake injectée par l'auto-injecteur.
/// Conserve la signature publique d'origine — `freq` annoncé par
/// `START_LISTEN` est utilisé comme suggestion mais peut être réécrit par
/// le scénario pour respecter les `freq_range` des règles.
fn fake_frame(module: Module, _freq: u32, tick: u64) -> RfFrame {
    let (scenario_freq, rssi, snr, payload_hex) = scenario_for(module, tick);
    RfFrame {
        module,
        freq: scenario_freq,
        rssi,
        snr,
        payload_hex,
        ts_ms: 0,
    }
}

// =============================================================================
//  Mapping probe → réponse vulnérable (M9-M12, pentest actif)
// =============================================================================
//
//  Quand l'app envoie un INJECT (= TxProbe d'une attaque), l'émulateur
//  reconnaît une poignée de signatures et répond avec une frame qui matche
//  le critère ListenFor de l'attaque correspondante. Cela permet de
//  démontrer une cible vulnérable sans matériel.
//
//  La table est volontairement courte mais couvre AU MOINS une attaque par
//  bande, de quoi déclencher un Finding par module en démo.

/// Inspecte un payload probe et, s'il correspond à un pattern connu,
/// renvoie `(module_de_la_réponse, freq_de_la_réponse, payload_de_la_réponse)`.
///
/// Le payload d'entrée est normalisé en uppercase sans espaces (les
/// catalogues TOML autorisent des espaces de lisibilité).
fn match_probe(payload_hex: &str) -> Option<(Module, u32, String)> {
    let p: String = payload_hex.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();

    // ----- WiFi (esp32_c6, 2.412 GHz) ---------------------------------------
    // WIFI-DEAUTH-PROBE  + WIFI-KRACK-REINSTALL-PROBE (réponse EAPoL msg-3)
    if p.starts_with("C0DEAD0DEAD0") || p.starts_with("08084B52") {
        return Some((Module::Esp32C6, 2_412_000_000, "888E0203AB12CD34EF".to_string()));
    }
    // WIFI-WPS-PIN-PROBE (réponse beacon WPS M1)
    if p.starts_with("80000000") && p.contains("DD0050F204") {
        return Some((Module::Esp32C6, 2_412_000_000, "DD0050F204104A0001".to_string()));
    }
    // WIFI-PMKID-CAPTURE-PROBE (réponse EAPoL avec PMKID)
    if p.starts_with("00003C00") {
        return Some((
            Module::Esp32C6,
            2_412_000_000,
            "888E02030075DEADBEEF0010CAFEBABE1234567890ABCDEFFEEDFACE".to_string(),
        ));
    }
    // WIFI-OPEN-AP-SCAN (réponse beacon Open)
    if p == "FF" {
        // Ambigu (utilisé aussi par RFID LF). Privilège LF, voir plus bas.
    }

    // ----- BLE (esp32_c6, 2.402 GHz) ----------------------------------------
    // BLE-JUST-WORKS-PROBE → SMP Pairing Response (opcode 02)
    if p.starts_with("010300000000") {
        return Some((Module::Esp32C6, 2_402_000_000, "02030000000000".to_string()));
    }
    // BLE-KNOB-ENTROPY-PROBE → LMP Accept (opcode 03)
    if p.starts_with("0F0101") {
        return Some((Module::Esp32C6, 2_402_000_000, "03010F".to_string()));
    }
    // BLE-BIAS-IMPERSONATION → LMP_sres (opcode 12)
    if p.starts_with("11AABBCCDDEEFF00") {
        return Some((Module::Esp32C6, 2_402_000_000, "12DEADBEEF".to_string()));
    }
    // BLE-GATT-ENUM → ATT Read By Group Type Response (opcode 11)
    if p.starts_with("100100FFFF2800") {
        return Some((Module::Esp32C6, 2_402_000_000, "11060100050000180100".to_string()));
    }
    // BLE-SWEYNTOOTH-LLE-TIMING → LL_TERMINATE_IND (opcode 0A)
    if p.starts_with("0414FF00FFFF") {
        return Some((Module::Esp32C6, 2_402_000_000, "0A02".to_string()));
    }

    // ----- NFC (st25r3916, 13.56 MHz) ---------------------------------------
    // NFC-DEFAULT-KEY-A
    if p.starts_with("6000FFFFFFFFFFFF") {
        return Some((Module::St25r3916, 13_560_000, "0A0000".to_string()));
    }
    // NFC-HARDNESTED-PROBE (réponse nested nonce avec préfixe E0)
    if p.starts_with("6110FFFFFFFFFFFF") {
        return Some((Module::St25r3916, 13_560_000, "E012345678".to_string()));
    }
    // NFC-MIFARE-NESTED (réponse nonce prédictible préfixe AB)
    if p.starts_with("61040123456789AB") {
        return Some((Module::St25r3916, 13_560_000, "ABCD1234".to_string()));
    }
    // NFC-ISO14443-ANTICOL : 9320 → UID 04…
    if p == "9320" {
        return Some((Module::St25r3916, 13_560_000, "04AABBCC".to_string()));
    }
    // NFC-UID-CLONE-PROBE : REQA 52 → ATQA 0004
    if p == "52" {
        return Some((Module::St25r3916, 13_560_000, "0004".to_string()));
    }

    // ----- Sub-GHz (sx1262, 433.92 MHz) -------------------------------------
    // SUBGHZ-FIXED-CODE-REPLAY : A1B2C3 → echo (ACK fixe)
    if p == "A1B2C3" {
        return Some((Module::Sx1262, 433_920_000, "A1B2C3".to_string()));
    }
    // SUBGHZ-JAM-LISTEN-REPLAY : FFFFFFFF puis attente d'une retx AABBCC…
    if p == "FFFFFFFF" {
        return Some((Module::Sx1262, 433_920_000, "AABBCC55".to_string()));
    }
    // SUBGHZ-RKE-BRUTEFORCE-WINDOW : 0000 / 0001 / 0002 → ack 7E…
    if p == "0000" || p == "0001" || p == "0002" {
        return Some((Module::Sx1262, 433_920_000, "7E0F0F".to_string()));
    }

    // ----- RFID LF (rfid_lf, 125 kHz) ---------------------------------------
    // RFID-LF-T5577-DETECT : 02 → config block
    if p == "02" {
        return Some((Module::RfidLf, 125_000, "0014804000000000".to_string()));
    }
    // RFID-LF-EM4102-READ / HID-PROX-READ / INDALA / AWID : tous démarrent
    // par FF (excitation porteuse). On répond par défaut un EM4102 plain.
    if p == "FF" {
        return Some((Module::RfidLf, 125_000, "AABBCCDDEE".to_string()));
    }

    None
}

// =============================================================================
//  Encodage des messages device → host
// =============================================================================
fn cap_msg(seq: u64) -> Value {
    let modules: Vec<ModuleInfo> = Module::ALL
        .iter()
        .enumerate()
        .map(|(i, m)| ModuleInfo {
            id:   u8::try_from(i).unwrap(),
            name: m.as_protocol_str().to_string(),
            caps: capabilities_for(*m),
        })
        .collect();
    serde_json::to_value(s::Message::Capabilities(Capabilities {
        seq,
        proto_version: s::PROTO_VERSION,
        fw_version: FW_VERSION.into(),
        hw: HW_NAME.into(),
        modules,
    })).unwrap()
}

fn ack(seq: u64) -> Value {
    serde_json::to_value(s::Message::Ack(Ack { seq, uptime_ms: None })).unwrap()
}

fn ack_with_uptime(seq: u64, uptime_ms: u64) -> Value {
    serde_json::to_value(s::Message::Ack(Ack { seq, uptime_ms: Some(uptime_ms) })).unwrap()
}

fn nack(seq: u64, reason: &str) -> Value {
    serde_json::to_value(s::Message::Nack(Nack { seq, reason: reason.into() })).unwrap()
}

fn frame_msg(f: &RfFrame) -> Value {
    serde_json::to_value(s::Message::Frame(f.clone())).unwrap()
}

fn tx_done_msg(seq: u64, module: Module, ts_ms: u64) -> Value {
    serde_json::to_value(s::Message::TxDone(s::TxDone { seq, module, ts_ms })).unwrap()
}

// =============================================================================
//  Helpers parsing
// =============================================================================
fn module_from_val(v: &Value) -> Option<Module> {
    serde_json::from_value(v.clone()).ok()
}

fn u32_from_val(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

// =============================================================================
//  Dispatcher — traite une requête, écrit potentiellement plusieurs réponses
// =============================================================================
async fn handle_line(
    line: &[u8],
    session: &Arc<Mutex<Session>>,
    out: &mut (impl AsyncWriteExt + Unpin),
) -> anyhow::Result<()> {
    let req: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(_) => return write_value(out, &nack(0, "parse")).await,
    };
    let seq = req.get("seq").and_then(Value::as_u64).unwrap_or(0);
    let Some(cmd) = req.get("cmd").and_then(Value::as_str) else {
        return write_value(out, &nack(seq, "no_cmd")).await;
    };

    let mut sess = session.lock().await;
    match cmd {
        "HANDSHAKE" => write_value(out, &cap_msg(seq)).await,

        "START_LISTEN" => {
            let Some(m) = req.get("module").and_then(module_from_val) else {
                return write_value(out, &nack(seq, "bad_module")).await;
            };
            let Some(f) = req.get("freq").and_then(u32_from_val) else {
                return write_value(out, &nack(seq, "bad_field")).await;
            };
            let idx = m as usize;
            sess.listening[idx] = true;
            sess.freqs[idx] = f;
            write_value(out, &ack(seq)).await
        }

        "STOP" => {
            let Some(m) = req.get("module").and_then(module_from_val) else {
                return write_value(out, &nack(seq, "bad_module")).await;
            };
            sess.listening[m as usize] = false;
            write_value(out, &ack(seq)).await
        }

        "INJECT" => {
            let Some(m) = req.get("module").and_then(module_from_val) else {
                return write_value(out, &nack(seq, "bad_module")).await;
            };
            let Some(freq) = req.get("freq").and_then(u32_from_val) else {
                return write_value(out, &nack(seq, "bad_field")).await;
            };
            let payload = req.get("payload_hex").and_then(Value::as_str).unwrap_or("");
            let rssi = req.get("rssi").and_then(Value::as_i64).unwrap_or(-60) as i16;
            let snr  = req.get("snr").and_then(Value::as_i64).unwrap_or(0) as i8;

            let frame = RfFrame {
                module: m,
                freq,
                rssi,
                snr,
                payload_hex: payload.to_uppercase(),
                ts_ms: sess.uptime_ms(),
            };
            // Le firmware fait un start_listen implicite si pas armé — on imite.
            let idx = m as usize;
            if !sess.listening[idx] {
                sess.listening[idx] = true;
                sess.freqs[idx] = freq;
            }
            // Push + drain immédiat (équivalent du drain_rx_frames du firmware).
            write_value(out, &frame_msg(&frame)).await?;
            write_value(out, &ack(seq)).await?;

            // ----- Réponse simulée à un probe d'attaque (mode pentest actif) -
            //  Si le payload injecté matche un pattern connu de
            //  `match_probe`, on émet ~50 ms plus tard une frame fake qui
            //  satisfait le critère ListenFor de l'attaque correspondante.
            //  Le `START_LISTEN` implicite ci-dessus garantit que la frame
            //  sera relayée par l'auto-injecteur du `frame_tx` côté client.
            if let Some((reply_mod, reply_freq, reply_payload)) =
                match_probe(&frame.payload_hex)
            {
                let reply = RfFrame {
                    module: reply_mod,
                    freq: reply_freq,
                    rssi: -55,
                    snr: 7,
                    payload_hex: reply_payload,
                    ts_ms: sess.uptime_ms() + 50,
                };
                // Assure que le module de réponse est armé pour que la UI
                // suive aussi (cohérent avec le flow START_LISTEN implicite).
                let ridx = reply_mod as usize;
                if !sess.listening[ridx] {
                    sess.listening[ridx] = true;
                    sess.freqs[ridx] = reply_freq;
                }
                // Petit délai pour rester réaliste — le ClientTask aura
                // déjà acked l'INJECT au moment où la trame réponse arrive.
                drop(sess); // libère le mutex avant le sleep
                tokio::time::sleep(Duration::from_millis(50)).await;
                return write_value(out, &frame_msg(&reply)).await;
            }
            Ok(())
        }

        "QUERY_FRAMES" => {
            let only = req.get("module").and_then(module_from_val);
            let max  = req.get("max").and_then(Value::as_u64).unwrap_or(64) as usize;
            let mut drained = 0;
            for (i, ring) in sess.rings.iter_mut().enumerate() {
                if drained >= max { break; }
                if let Some(o) = only {
                    if (o as usize) != i { continue; }
                }
                while drained < max {
                    let Some(f) = ring.pop_front() else { break; };
                    write_value(out, &frame_msg(&f)).await?;
                    drained += 1;
                }
            }
            write_value(out, &ack(seq)).await
        }

        "TX" => {
            let Some(m) = req.get("module").and_then(module_from_val) else {
                return write_value(out, &nack(seq, "bad_module")).await;
            };
            let ts = sess.uptime_ms();
            write_value(out, &ack(seq)).await?;
            write_value(out, &tx_done_msg(seq, m, ts)).await
        }

        "STATUS" => write_value(out, &ack_with_uptime(seq, sess.uptime_ms())).await,

        "RESET" => {
            for r in &mut sess.rings { r.clear(); }
            sess.listening = [false; 5];
            sess.freqs = [0; 5];
            write_value(out, &ack(seq)).await
        }

        _ => write_value(out, &nack(seq, "unknown_cmd")).await,
    }
}

async fn write_value(
    out: &mut (impl AsyncWriteExt + Unpin),
    v: &Value,
) -> anyhow::Result<()> {
    let line = codec::encode_line(v).map_err(|e| anyhow::anyhow!("encode: {e}"))?;
    out.write_all(&line).await?;
    out.flush().await?;
    Ok(())
}

// =============================================================================
//  Auto-injecteur
// =============================================================================
async fn auto_inject_loop(
    session: Arc<Mutex<Session>>,
    out: Arc<Mutex<BufWriter<tokio::io::WriteHalf<TcpStream>>>>,
    period_ms: u64,
) {
    if period_ms == 0 { return; }
    let mut tick = interval(Duration::from_millis(period_ms));
    let mut counter: u64 = 0;
    loop {
        tick.tick().await;
        counter = counter.wrapping_add(1);
        let snapshot = {
            let sess = session.lock().await;
            Module::ALL
                .iter()
                .enumerate()
                .filter_map(|(i, m)| {
                    if sess.listening[i] { Some((*m, sess.freqs[i], sess.uptime_ms())) }
                    else { None }
                })
                .collect::<Vec<_>>()
        };
        if snapshot.is_empty() { continue; }
        let mut w = out.lock().await;
        for (m, f, ts) in snapshot {
            let mut frame = fake_frame(m, f, counter);
            frame.ts_ms = ts;
            if write_value(&mut *w, &frame_msg(&frame)).await.is_err() {
                return;
            }
        }
    }
}

// =============================================================================
//  Per-connection driver
// =============================================================================
async fn handle_connection(
    stream: TcpStream,
    peer: std::net::SocketAddr,
    auto_inject_ms: u64,
) -> anyhow::Result<()> {
    tracing::info!(?peer, "new client connected");
    stream.set_nodelay(true)?;
    let (rh, wh) = tokio::io::split(stream);
    let mut reader = BufReader::with_capacity(2048, rh);
    let writer = Arc::new(Mutex::new(BufWriter::with_capacity(2048, wh)));
    let session = Arc::new(Mutex::new(Session::new()));

    // Auto-injecteur en task séparée — vivra jusqu'à la déconnexion.
    let session_for_task = session.clone();
    let writer_for_task = writer.clone();
    let inject_task = tokio::spawn(async move {
        auto_inject_loop(session_for_task, writer_for_task, auto_inject_ms).await;
    });

    let mut line = Vec::new();
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line).await?;
        if n == 0 { break; }
        // Strip trailing \r\n.
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.is_empty() { continue; }
        let mut w = writer.lock().await;
        if let Err(e) = handle_line(&line, &session, &mut *w).await {
            tracing::warn!(?peer, ?e, "handler error");
            break;
        }
    }
    inject_task.abort();
    tracing::info!(?peer, "client disconnected");
    Ok(())
}

// =============================================================================
//  main
// =============================================================================
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let listener = TcpListener::bind(&args.listen).await?;
    tracing::info!(listen = %args.listen, "cyberdeck-emu listening");

    loop {
        let (stream, peer) = listener.accept().await?;
        let ms = args.auto_inject_ms;
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer, ms).await {
                tracing::warn!(?e, "connection ended with error");
            }
        });
    }
}

// =============================================================================
//  Helpers d'imports — request module est volontairement importé pour
//  rappeler que côté client on construit les requêtes via
//  `cyberdeck_api::proto::request::*`. Inutile ici (l'émulateur PARSE des
//  requêtes, ne les construit pas) — on supprime l'import si dead.
// =============================================================================
#[allow(dead_code)]
fn _request_link() { let _ = request::status(0); }
