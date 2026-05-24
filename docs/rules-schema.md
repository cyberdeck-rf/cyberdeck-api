# Schéma TOML du Rules Pack — `wraith-rules`

Ce document décrit le format TOML utilisé par le moteur de règles pentest
embarqué dans `cyberdeck-api`. Il s'adresse aux contributeurs externes qui
veulent **proposer une règle**, et au mainteneur qui publie un pack distant
téléchargeable à chaud via HTTPS.

Le pack par défaut (15 règles) est embarqué dans le binaire via
`include_str!` (cf. `crates/core/src/rules/catalog/default_rules.toml`).
Un updater HTTPS (`crates/core/src/rules/remote.rs`) peut remplacer ce pack
à chaud, sans recompiler l'app.

---

## 1. Préambule

Le rôle de ce schéma est de décrire **déclarativement** les indicateurs
qu'un RF cyberdeck doit chercher dans les trames qu'il reçoit (sub-GHz, NFC,
WiFi, BLE, 2G4-LoRa, 125 kHz). Une règle peut :

- inspecter une trame isolée (matchers **stateless**) — ex. fréquence,
  longueur, motif hexa, regex sur le payload ;
- agréger plusieurs trames sur une fenêtre glissante (matchers **stateful**)
  — ex. trois trames identiques d'affilée, rolling code immobile, burst
  inhabituel.

Quand une règle matche, le moteur émet un `Finding` (id, severity, frame
source, timestamp) qui remonte dans le flux applicatif `findings_stream`
exposé à l'UI Flutter / au CLI.

Toutes les chaînes du pack sont en clair côté hôte. L'obfuscation des
littéraux est une protection firmware-only — cf. `feedback_obfuscation`.

---

## 2. Champs top-level

| Clé              | Type     | Obligatoire | Description |
|------------------|----------|-------------|-------------|
| `schema_version` | `u32`    | oui         | Version du schéma du pack (actuellement `1`). Bump si breaking. |
| `pack_version`   | `String` | oui         | Version sémantique ou datée du pack lui-même (ex. `"2026.05.21"`). |
| `source`         | `String` | oui         | Origine du pack (`"default"`, `"github:anthony/wraith-rules"`, `"local"`). |
| `[[rule]]`       | tableau  | oui (≥ 1)   | Tableau de règles ; un pack avec 0 règle est refusé par l'updater. |

Exemple minimal :

```toml
schema_version = 1
pack_version   = "2026.05.21"
source         = "default"

[[rule]]
id       = "DEMO-001"
title    = "Démo"
severity = "info"
modules  = ["sx1262"]
[rule.match]
all = [ { payload_length_between = [0, 64] } ]
```

---

## 3. Structure d'une règle

Chaque entrée `[[rule]]` admet les champs suivants :

| Champ            | Type            | Obligatoire | Description |
|------------------|-----------------|-------------|-------------|
| `id`             | `String`        | oui         | Identifiant stable au format `<BAND>-<NNN>` (ex. `SUBGHZ-001`). |
| `title`          | `String`        | oui         | Titre court affiché à l'UI (< 80 caractères). |
| `severity`       | enum            | oui         | Voir [§ 5 Sévérités](#5-sévérités). |
| `modules`        | `[String]`      | oui         | Liste des modules cibles (`sx1262`, `st25r3916`, `esp32c6`, `sx1280`, `rfid_lf`). |
| `description`    | `String`        | non         | Description longue (multi-ligne `"""…"""` recommandé). |
| `recommendation` | `String`        | non         | Conseil utilisateur final, **en français**. |
| `references`     | `[String]`      | non         | CVE, papers academic, cheatsheet projet. |
| `window_frames`  | `u32`           | non         | Taille de fenêtre glissante (frames) pour les matchers stateful. |
| `[rule.match]`   | objet           | oui         | Bloc matcher — voir [§ 4](#4-bloc-rulematch). |

`BAND` ∈ {`SUBGHZ`, `NFC`, `RFID-LF`, `WIFI`, `BLE`, `LORA`}.

---

## 4. Bloc `[rule.match]`

Le bloc `[rule.match]` accepte **exactement une** des clés suivantes :

- `all = [ matcher, matcher, … ]` — conjonction logique (tous doivent matcher).
- `any = [ matcher, matcher, … ]` — disjonction logique (au moins un).
- `stateful = "<nom_operateur>"` — un matcher d'historique (cf. § 4.2).

Les compositions (`all` imbriqué dans `any`, etc.) sont **non supportées en
v1** — utiliser deux règles distinctes si nécessaire.

### 4.1 Matchers stateless

| Opérateur                  | Syntaxe TOML                                              | Sémantique |
|----------------------------|-----------------------------------------------------------|------------|
| `freq_range`               | `{ freq_range = [LO_HZ, HI_HZ] }`                         | `LO_HZ ≤ frame.freq ≤ HI_HZ` |
| `freq_eq`                  | `{ freq_eq = 868_100_000 }`                               | Égalité exacte |
| `payload_length_between`   | `{ payload_length_between = [MIN, MAX] }` (octets)        | Bornes incluses |
| `payload_length_eq`        | `{ payload_length_eq = N }`                               | Égalité (octets) |
| `payload_hex_starts_with`  | `{ payload_hex_starts_with = "DEADBEEF" }`                | Prefix hex (majuscules) |
| `payload_hex_ends_with`    | `{ payload_hex_ends_with = "CAFE" }`                      | Suffix hex |
| `payload_hex_matches`      | `{ payload_hex_matches = "^[A-F0-9]{8,}$" }`              | Regex sur la string hex |
| `rssi_above`               | `{ rssi_above = -60 }`                                    | `frame.rssi > VAL` (dBm) |
| `rssi_below`               | `{ rssi_below = -90 }`                                    | `frame.rssi < VAL` (dBm) |
| `snr_above`                | `{ snr_above = 6 }`                                       | `frame.snr > VAL` (dB) |
| `module_eq`                | `{ module_eq = "sx1262" }`                                | Restreint à ce module |

### 4.2 Matchers stateful

Activés via `stateful = "<nom>"`, et tirent leur fenêtre du champ
`window_frames` au niveau de la règle. Tous opèrent sur le buffer FIFO
maintenu par `RulesEngine` par `(module, freq)`.

| Opérateur                       | Sémantique |
|---------------------------------|------------|
| `payload_identical_in_window`   | Les N dernières frames du module ont un payload identique (réplay candidate). |
| `rolling_code_immobile`         | Idem ci-dessus restreint au sub-GHz (alias sémantique). |
| `burst_frequency_above`         | Compte de frames sur la fenêtre > seuil (configuré dans le code engine). |
| `unique_payload_count_below`    | Sur N frames, ≤ K payloads distincts (replay multi-trame). |

---

## 5. Sévérités

| Valeur     | Sens UI       | Couleur recommandée |
|------------|---------------|---------------------|
| `info`     | Information   | gris                |
| `low`      | Faible        | bleu                |
| `medium`   | Moyenne       | jaune               |
| `high`     | Élevée        | orange              |
| `critical` | Critique      | rouge               |

L'UI peut filtrer par sévérité ; les `info` sont masquées par défaut dans
la mobile-app.

---

## 6. Exemple complet

```toml
[[rule]]
id             = "SUBGHZ-001"
title          = "Rolling code non-changeant — replay possible"
severity       = "critical"
modules        = ["sx1262"]
window_frames  = 3
description    = """
Trois trames consécutives identiques observées sur le même module sub-GHz. \
Un rolling code (KeeLoq, Hi-Tag2, etc.) DOIT changer à chaque appui — un \
payload constant indique soit un fixed-code (vulnérable au replay immédiat) \
soit un dysfonctionnement crypto exploitable.
"""
recommendation = "Auditer l'algorithme de rolling code du transmetteur. Remplacer par AES-CCM."
references     = [
  "Kamkar — RollJam, DEF CON 23 (2015)",
  "Ibrahim et al. — Jam-Listen-Replay on RKE, 2019",
]
[rule.match]
stateful = "payload_identical_in_window"
```

---

## 7. Comment publier une mise à jour

L'updater HTTPS (`rules::remote::fetch_remote_pack`) télécharge le pack
depuis une URL configurable.

1. **URL** : variable d'environnement `WRAITH_RULES_URL`. Par défaut :
   `https://raw.githubusercontent.com/anthony/wraith-rules/main/rules.toml`.
2. **Format** : TOML strict, charset UTF-8, terminateur LF.
3. **Contrôle d'intégrité** :
   - L'updater calcule un **SHA-256** sur le body et le compare au cache
     local (`<cache_dir>/rules-pack/pack.sha256`). Si identique →
     `updated = false`, pas de remplacement.
   - L'updater envoie l'**ETag** précédent dans `If-None-Match`. Le serveur
     peut répondre `304 Not Modified` pour économiser la bande passante.
4. **Validation** : un pack qui parse mais contient `rules.len() == 0` est
   **refusé** (cache courant préservé).
5. **Persistance** : le cache est écrit de façon atomique
   (write-then-rename via `tempfile`) pour éviter une corruption si le
   process meurt en plein download.

### Workflow contributeur

```text
fork → edit rules.toml → PR → review →
  merge main → cdn invalidate →
    next fetch_remote_pack() returns updated=true
```

### Workflow client

```rust
use cyberdeck_api::rules::remote::fetch_remote_pack;

let res = fetch_remote_pack(None).await?;
if res.updated {
    if let Some(pack) = res.pack {
        engine.replace_pack(pack);
        tracing::info!(
            version = %res.pack_version,
            count   = res.rules_count,
            "rules pack mis à jour"
        );
    }
}
```

### En cas de panne réseau

Le boot offline fonctionne : le pack par défaut compilé reste actif.
L'updater logge l'erreur ; aucune perturbation côté UI.
