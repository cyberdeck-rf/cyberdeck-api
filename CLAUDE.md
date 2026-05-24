# cyberdeck-api — CLAUDE.md

Bibliothèque Rust + CLI binaire + **émulateur firmware dockerisé** pour parler au RF Cyberdeck. Frère du repo `cyberdeck/` (firmware bare-metal STM32) et de `mobile-app/` (UI Tauri 2.0). Suit le plan canonique `~/.claude/plans/oui-a-clignote-bien-cheerful-snowglobe.md`.

## Trois crates

| Crate | Rôle |
|---|---|
| `cyberdeck-api` (`crates/core`) | Lib partagée : Transport trait (Serial + TCP), codec NDJSON, façade `Client`, 5 plugins |
| `cyberdeck-cli` (`crates/cli`) | Binaire desktop : `status`, `scan`, `inject`, `list-ports`. Flag `--port` ou `--tcp host:port` |
| `cyberdeck-emu` (`crates/emu`) | Émulateur firmware sur TCP — dockerisable, permet de bosser sans matériel |

## Émulateur (cyberdeck-emu)

Quand la F4-Disco n'est pas branchée (ou pas dispo, ou la stack USB-CDC est encore en debug), on lance l'émulateur :

```bash
# Option A : Docker (recommandé pour la démo)
cd ~/Downloads/mission/cyberdeck-api
docker compose up -d
# expose 17017/tcp ; même protocole NDJSON v1 que le vrai firmware

# Option B : binaire natif (dev local rapide)
cargo run -p cyberdeck-emu
```

Puis le CLI ou la mobile-app se connecte via TCP :

```bash
cyberdeck-cli --tcp 127.0.0.1:17017 status
cyberdeck-cli --tcp 127.0.0.1:17017 scan sx1262 868100000 --duration 10
```

L'émulateur a un **auto-injecteur** intégré : sur chaque module en écoute, il pousse une fake frame par seconde (configurable via `--auto-inject-ms`), ce qui donne une UI vivante en démo sans matériel ni script extérieur.

---

## Rôle

Le firmware est volontairement minimal — il fait juste passer les données RF entre l'antenne et l'USB (protocole JSON v1 sur USB-CDC ou Vendor Bulk). Toute l'intelligence applicative (décodage protocoles, analyse, persistance, attaques) vit ici, côté hôte, dans la bibliothèque Rust `cyberdeck-api` avec des plugins par bande RF.

```
┌─────────────────────┐
│ cyberdeck-cli       │  → CLI desktop (scan, inject, replay, capture)
└────────┬────────────┘
         │
┌────────▼────────────┐
│ cyberdeck-api/core  │  → Transport + protocol codec + plugins + SQLite
└────────┬────────────┘
         │ NDJSON sur USB-CDC (puis Vendor Bulk plus tard)
┌────────▼────────────┐
│ Firmware cyberdeck  │  → pont transparent vers les 5 transceivers RF
└─────────────────────┘
```

L'app mobile (`mobile-app/` via Tauri 2.0) **embarque la même crate `cyberdeck-api/core`** dans son `src-tauri`. Zéro duplication entre desktop et mobile.

---

## Contraintes durables

| Aspect | Décision |
|---|---|
| **Langage** | Rust edition 2024 (rustc ≥ 1.85) |
| **Async runtime** | tokio (rt-multi-thread + macros + io-util) |
| **Transports dispos** | (a) tokio-serial sur USB-CDC, feature `serial` (default-on), (b) tokio TcpStream vers `cyberdeck-emu`, toujours dispo |
| **Transport v2** | nusb (Vendor Bulk via USB3300 ULPI) quand le PCB H743 sera dispo |
| **Codec** | NDJSON (`tokio_util::codec::LinesCodec`) — un objet JSON par ligne, terminateur `\n`, max 1024 octets/ligne |
| **Protocole** | miroir 1:1 de `cyberdeck/src/proto/schema.hpp` + `commands.hpp` |
| **Plugins** | un par bande RF (sx1262, st25r3916, esp32c6, sx1280, rfid_lf), implémentent un trait `Plugin` commun |
| **Persistance** | SQLite via rusqlite (à ajouter en M4) |
| **CLI** | clap derive + comfy-table + indicatif |

---

## Arborescence

```
cyberdeck-api/
├── Cargo.toml            workspace manifest
├── CLAUDE.md             ← ce fichier
├── .gitignore
├── crates/
│   ├── core/             cyberdeck-api crate (lib)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            prelude + ré-exports
│   │       ├── error.rs          thiserror::Error
│   │       ├── proto/
│   │       │   ├── mod.rs
│   │       │   ├── schema.rs     enums Cmd, Module + structs Capabilities, RfFrame, etc.
│   │       │   ├── request.rs    helpers pour construire les payloads host→device
│   │       │   ├── message.rs    parsing des réponses device→host
│   │       │   └── codec.rs      NDJSON framer
│   │       ├── transport/
│   │       │   ├── mod.rs        Transport trait async
│   │       │   ├── serial.rs     impl tokio-serial (USB-CDC)
│   │       │   └── usb_bulk.rs   [stub] Vendor Bulk via nusb — phase 2
│   │       ├── client.rs         Client<T> façade async
│   │       ├── plugins/
│   │       │   ├── mod.rs        Plugin trait + registry
│   │       │   ├── sub_ghz.rs    SX1262 — LoRa, FSK, attaques RollJam/Jam-Listen-Replay
│   │       │   ├── nfc.rs        ST25R3916 — Mifare Classic, ISO14443/15693, Hardnested
│   │       │   ├── rfid_lf.rs    125 kHz — EM4102, HID Prox, T5577 emulation
│   │       │   ├── wifi_ble.rs   ESP32-C6 — WiFi 6, BLE, Zigbee, KRACK, KNOB
│   │       │   └── lora_2g4.rs   SX1280 — ELRS, BlackoutADR
│   │       └── session/          [M4]
│   │           ├── mod.rs        Session, CaptureEntry
│   │           └── store.rs      SQLite via rusqlite
│   └── cli/              cyberdeck-cli binary
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs           clap parse + tracing setup
│           └── commands/
│               ├── status.rs
│               ├── scan.rs
│               ├── inject.rs
│               ├── capture.rs    [M4]
│               └── replay.rs     [M4]
└── docs/
    └── protocol-v1.md            référence pour le jury soutenance
```

---

## Commandes utiles

```bash
# Build workspace
cargo build --workspace

# Tests
cargo test --workspace

# CLI (auto-détecte le port série STM32)
cargo run -p cyberdeck-cli -- status
cargo run -p cyberdeck-cli -- scan sx1262 868100000
cargo run -p cyberdeck-cli -- inject sx1262 868100000 --payload DEADBEEF

# Release
cargo build --workspace --release  # binaire stripé dans target/release/cyberdeck-cli
```

---

## Synchronisation protocole avec le firmware

Le firmware (cyberdeck/src/proto/schema.hpp + commands.hpp) est le **source de vérité** du protocole. Toute évolution doit être faite en parallèle dans les deux repos. Le test d'intégration `tests/protocol_compat.rs` (à venir) compare les noms de commandes/modules entre les deux côtés.
