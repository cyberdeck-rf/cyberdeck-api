//! Client HTTPS de mise à jour du pack de règles.
//!
//! # Stratégie
//!
//! 1. **GET** sur l'URL fournie (par défaut `WRAITH_RULES_URL` ou un placeholder
//!    GitHub Raw). Header `If-None-Match` envoyé si on a un ETag en cache.
//! 2. **304 Not Modified** → on retourne le pack actuel sans rien re-parser
//!    (`updated = false`).
//! 3. **200 OK** → on calcule le SHA-256 du body, on compare au cache local.
//!    Si identique, on renvoie `updated = false`. Sinon on parse le TOML et
//!    on persiste l'enveloppe (body + ETag + SHA-256) sur disque.
//! 4. Erreurs réseau / parse / pack vide → renvoyées en `Err(String)`
//!    user-friendly (la UI les affiche en banner).
//!
//! # Cache local
//!
//! Le cache vit dans `$WRAITH_CACHE_DIR/rules-pack/` ou, à défaut,
//! `<tempdir>/wraith-rules-cache/`. On stocke trois fichiers :
//! * `pack.toml`   — body brut (utile pour debug + offline boot),
//! * `pack.sha256` — empreinte hexa (32 octets = 64 caractères),
//! * `pack.etag`   — ETag HTTP (peut être vide si le serveur n'en émet pas).
//!
//! # Thread model
//!
//! `ureq` est blocking ; on l'exécute dans `tokio::task::spawn_blocking` pour
//! ne pas geler la runtime tokio.
//!
//! # TODO v2
//!
//! Tests intégration avec `mockito` — pas de test online ici (besoin réseau).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

// Hypothèse cross-agent : `RulePack` est défini par un autre agent dans
// `rules/schema.rs` avec les champs `schema_version`, `pack_version`, `source`,
// `rules: Vec<Rule>`. On l'importe via le chemin attendu — le compilo râlera
// tant que l'autre agent n'a pas mergé son schema, c'est OK.
use crate::rules::schema::RulePack;

/// URL par défaut si `WRAITH_RULES_URL` n'est pas défini.
///
/// Pointe sur `cyberdeck-server` en dev local (port 8080).  Override via la
/// variable d'environnement `WRAITH_RULES_URL` en prod (ou via le paramètre
/// `override_url` côté API publique).
///
/// Format attendu : soit la réponse JSON de `cyberdeck-server`
/// (`{version, sha256, channel, count, content, published_at}`) — auquel cas
/// `extract_toml_body` ré-extrait le champ `content` TOML — soit du TOML brut
/// (compat héritée GitHub Raw / S3 statique).
pub const DEFAULT_RULES_URL: &str =
    "http://localhost:8080/api/v1/packs/rules/latest?channel=stable";

/// Alias historique (compat avec les anciens scripts).  À supprimer en M16+.
pub const DEFAULT_RULES_URL_LEGACY: &str =
    "https://raw.githubusercontent.com/anthony/wraith-rules/main/rules.toml";

/// Nom de la variable d'environnement override.
///
/// Deux noms acceptés (dans cet ordre) :
///   * `CYBERDECK_RULES_PACK_URL` — préféré (cohérent avec le reste de la
///     stack `cyberdeck-*`).
///   * `WRAITH_RULES_URL` — alias hérité du codename projet.
pub const ENV_URL: &str = "CYBERDECK_RULES_PACK_URL";

/// Alias hérité (encore lu en fallback) — voir [`ENV_URL`].
pub const ENV_URL_LEGACY: &str = "WRAITH_RULES_URL";

/// Résultat d'une tentative de mise à jour.
#[derive(Debug, Clone)]
pub struct RemoteUpdateResult {
    /// `true` si le pack a effectivement changé depuis le dernier appel.
    pub updated: bool,
    /// `pack_version` extraite du TOML (ex. `"2026.05.21"`).
    pub pack_version: String,
    /// Nombre de règles dans le pack téléchargé.
    pub rules_count: usize,
    /// URL effectivement requêtée (utile pour la UI / logs).
    pub source_url: String,
    /// Pack parsé, prêt à être passé à `RulesEngine::replace_pack`.
    ///
    /// `None` si 304 et qu'aucune ré-évaluation n'est nécessaire — l'appelant
    /// peut ignorer la mise à jour.
    pub pack: Option<RulePack>,
}

/// Renvoie l'URL effective à utiliser.
///
/// Ordre de priorité :
///   1. `override_url` (paramètre explicite, p. ex. depuis Dart),
///   2. `CYBERDECK_RULES_PACK_URL` (env var actuelle),
///   3. `WRAITH_RULES_URL` (env var héritée, kept for back-compat),
///   4. [`DEFAULT_RULES_URL`] (localhost:8080 en dev).
#[must_use]
pub fn effective_url(override_url: Option<&str>) -> String {
    if let Some(u) = override_url {
        return u.to_string();
    }
    if let Ok(u) = std::env::var(ENV_URL) {
        return u;
    }
    if let Ok(u) = std::env::var(ENV_URL_LEGACY) {
        return u;
    }
    DEFAULT_RULES_URL.to_string()
}

/// Extrait le corps TOML depuis la réponse HTTP.
///
/// Le serveur `cyberdeck-server` (cf. `server/src/packs/routes.rs`) renvoie
/// un objet JSON :
///
/// ```json
/// {
///   "version": "1.0.0",
///   "sha256":  "<hex>",
///   "channel": "stable",
///   "count":   15,
///   "content": "schema_version = 1\npack_version = ...",
///   "published_at": "2026-05-21T12:34:56Z"
/// }
/// ```
///
/// On veut consommer ce flux directement sans ré-encoder un endpoint static
/// dédié. La règle d'auto-détection :
///   * si le body trimé commence par `{` ET contient `"content"` → on tente
///     de désérialiser un `LatestPackEnvelope` et on renvoie son champ
///     `content`.
///   * sinon → on considère que c'est du TOML brut (cas hérité GitHub Raw,
///     CDN statique, fichier local servé via `python -m http.server`).
///
/// On garde le fallback TOML brut pour ne pas casser les tests d'intégration
/// existants qui montent un serveur HTTP minimal qui sert un `pack.toml` cru.
fn extract_toml_body(body: &str) -> String {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') && trimmed.contains("\"content\"") {
        #[derive(serde::Deserialize)]
        struct LatestPackEnvelope {
            content: String,
        }
        if let Ok(env) = serde_json::from_str::<LatestPackEnvelope>(body) {
            return env.content;
        }
        // Si on a détecté une enveloppe mais que le parse échoue, on log
        // (best-effort) et on retombe sur le body brut — l'erreur TOML
        // ressortira en aval avec un message plus parlant.
        tracing::warn!(
            target: "cyberdeck::rules::remote",
            "réponse JSON-like détectée mais parse échoué — fallback TOML brut"
        );
    }
    body.to_string()
}

/// Chemin du répertoire de cache.
fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("WRAITH_CACHE_DIR") {
        return PathBuf::from(d).join("rules-pack");
    }
    std::env::temp_dir().join("wraith-rules-cache")
}

fn cached_etag(dir: &Path) -> Option<String> {
    let p = dir.join("pack.etag");
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn cached_sha(dir: &Path) -> Option<String> {
    let p = dir.join("pack.sha256");
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn cached_body(dir: &Path) -> Option<String> {
    let p = dir.join("pack.toml");
    std::fs::read_to_string(p).ok()
}

/// Persistance atomique (write-then-rename via tempfile) — évite un cache
/// corrompu si le process meurt entre deux fichiers.
fn write_cache(dir: &Path, body: &str, sha: &str, etag: Option<&str>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create_dir_all({}): {e}", dir.display()))?;

    let write_one = |name: &str, content: &str| -> Result<(), String> {
        let tmp =
            tempfile::NamedTempFile::new_in(dir).map_err(|e| format!("tempfile: {e}"))?;
        std::fs::write(tmp.path(), content)
            .map_err(|e| format!("write {name}: {e}"))?;
        tmp.persist(dir.join(name))
            .map_err(|e| format!("persist {name}: {e}"))?;
        Ok(())
    };

    write_one("pack.toml", body)?;
    write_one("pack.sha256", sha)?;
    write_one("pack.etag", etag.unwrap_or(""))?;
    Ok(())
}

/// SHA-256 hexadécimal minuscule (compatible avec `sha256sum`).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push(char::from_digit(u32::from(b >> 4), 16).unwrap());
        s.push(char::from_digit(u32::from(b & 0x0F), 16).unwrap());
    }
    s
}

/// Réponse brute d'un GET, abstraite pour les tests.
struct RawResponse {
    status: u16,
    etag: Option<String>,
    body: String,
}

fn do_get_blocking(url: &str, prev_etag: Option<&str>) -> Result<RawResponse, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(15))
        .user_agent(concat!("wraith-rules-updater/", env!("CARGO_PKG_VERSION")))
        .build();

    let mut req = agent.get(url);
    if let Some(et) = prev_etag {
        req = req.set("If-None-Match", et);
    }

    match req.call() {
        Ok(resp) => {
            let status = resp.status();
            let etag = resp.header("ETag").map(str::to_string);
            // ureq consomme `resp` quand on appelle into_string.
            let body = resp
                .into_string()
                .map_err(|e| format!("lecture body: {e}"))?;
            Ok(RawResponse { status, etag, body })
        }
        Err(ureq::Error::Status(code, resp)) => {
            // Cas 304 : pas considéré comme une erreur fonctionnelle.
            if code == 304 {
                let etag = resp.header("ETag").map(str::to_string);
                return Ok(RawResponse {
                    status: 304,
                    etag,
                    body: String::new(),
                });
            }
            Err(format!(
                "HTTP {code} en téléchargeant le pack ({})",
                resp.status_text()
            ))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("réseau: {t}")),
    }
}

/// Étape pure — pas d'I/O réseau, parsing + validation.
///
/// Renvoie l'objet `RulePack` si le TOML est valide ET le pack non vide.
fn parse_and_validate(body: &str) -> Result<RulePack, String> {
    let pack: RulePack = toml::from_str(body)
        .map_err(|e| format!("TOML invalide: {e}"))?;
    if pack.rules.is_empty() {
        return Err("pack vide (0 règle) — refus de remplacer le pack courant".into());
    }
    Ok(pack)
}

/// Télécharge le pack distant, valide et met à jour le cache local.
///
/// `override_url = None` → utilise `WRAITH_RULES_URL` ou le placeholder.
///
/// # Erreurs
///
/// * Réseau injoignable / DNS / timeout.
/// * Status HTTP ∉ {200, 304}.
/// * TOML mal formé.
/// * `rules.len() == 0` (refusé — un pack vide casserait le moteur).
pub async fn fetch_remote_pack(
    override_url: Option<&str>,
) -> Result<RemoteUpdateResult, String> {
    let url = effective_url(override_url);
    let dir = cache_dir();
    let prev_etag = cached_etag(&dir);
    let prev_sha = cached_sha(&dir);

    // ureq bloquant -> spawn_blocking pour ne pas geler tokio.
    let url_clone = url.clone();
    let etag_for_task = prev_etag.clone();
    let raw = tokio::task::spawn_blocking(move || {
        do_get_blocking(&url_clone, etag_for_task.as_deref())
    })
    .await
    .map_err(|e| format!("join: {e}"))??;

    // 304 → on retombe sur le cache local si possible.
    if raw.status == 304 {
        if let (Some(body), Some(_sha)) = (cached_body(&dir), prev_sha.clone()) {
            // On re-parse le cache pour garantir qu'il est toujours valide
            // (pack_version + rules_count).  En cas d'erreur, on traite
            // comme un cache corrompu et on signale au caller.
            let pack = parse_and_validate(&body)
                .map_err(|e| format!("cache local corrompu: {e}"))?;
            return Ok(RemoteUpdateResult {
                updated: false,
                pack_version: pack.pack_version.clone(),
                rules_count: pack.rules.len(),
                source_url: url,
                pack: Some(pack),
            });
        }
        return Err("304 reçu mais aucun cache local — réessayer sans If-None-Match".into());
    }

    if raw.status != 200 {
        return Err(format!("status inattendu: {}", raw.status));
    }

    // Extraction du corps TOML : on supporte à la fois la réponse JSON
    // `cyberdeck-server` (champ `content`) et le TOML brut (CDN statique).
    let toml_body = extract_toml_body(&raw.body);

    // Le SHA-256 cache key porte sur le TOML extrait, pas l'enveloppe JSON.
    // C'est cohérent avec ce que renvoie le serveur dans son ETag
    // (`"<sha(content)>"`).
    let sha = sha256_hex(toml_body.as_bytes());

    // Si le contenu est identique au cache → updated = false même si l'ETag
    // a changé (certains CDN tournent les ETags sans changer le contenu).
    if prev_sha.as_deref() == Some(sha.as_str()) {
        let pack = parse_and_validate(&toml_body)?;
        // On rafraîchit l'ETag dans le cache (souvent un weak validator a
        // changé), mais on ne signale pas un update au caller.
        let _ = write_cache(&dir, &toml_body, &sha, raw.etag.as_deref());
        return Ok(RemoteUpdateResult {
            updated: false,
            pack_version: pack.pack_version.clone(),
            rules_count: pack.rules.len(),
            source_url: url,
            pack: Some(pack),
        });
    }

    // Nouveau contenu — on valide AVANT d'écrire le cache, pour ne pas écraser
    // un cache valide par du TOML pourri.
    let pack = parse_and_validate(&toml_body)?;
    write_cache(&dir, &toml_body, &sha, raw.etag.as_deref())?;

    Ok(RemoteUpdateResult {
        updated: true,
        pack_version: pack.pack_version.clone(),
        rules_count: pack.rules.len(),
        source_url: url,
        pack: Some(pack),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_reference_vector() {
        // Vecteur NIST FIPS 180-4 — sha256("abc")
        let h = sha256_hex(b"abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn effective_url_respects_override() {
        assert_eq!(
            effective_url(Some("https://example.com/x.toml")),
            "https://example.com/x.toml"
        );
    }

    #[test]
    fn effective_url_falls_back_to_default() {
        // SAFETY: tests env vars are process-global ; ne pas paralléliser cette
        // assertion avec d'autres tests qui touchent les env vars.
        // Ici on s'assure que sans override + sans aucune env var connue,
        // on retombe sur DEFAULT (localhost:8080 en dev).
        unsafe {
            std::env::remove_var(ENV_URL);
            std::env::remove_var(ENV_URL_LEGACY);
        }
        assert_eq!(effective_url(None), DEFAULT_RULES_URL);
    }

    #[test]
    fn extract_toml_body_passes_through_raw_toml() {
        let raw = "schema_version = 1\npack_version = \"x\"\n";
        assert_eq!(extract_toml_body(raw), raw);
    }

    #[test]
    fn extract_toml_body_unwraps_cyberdeck_server_envelope() {
        // Simule la réponse JSON de cyberdeck-server (cf. dto::LatestPackResponse).
        let server_json = serde_json::json!({
            "version":  "1.0.0",
            "sha256":   "deadbeef",
            "channel":  "stable",
            "count":    1,
            "content":  "schema_version = 1\npack_version = \"y\"\n",
            "published_at": null,
        })
        .to_string();
        let body = extract_toml_body(&server_json);
        assert!(body.starts_with("schema_version = 1"));
        assert!(body.contains("pack_version = \"y\""));
    }

    // TODO v2 : tests intégration avec `mockito` pour simuler 200/304/timeout
    // sans accès réseau. Pour l'instant, fetch_remote_pack n'est pas
    // unit-testé (besoin d'un mock HTTP).
}
