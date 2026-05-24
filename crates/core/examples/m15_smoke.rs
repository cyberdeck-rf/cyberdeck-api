// =============================================================================
//  m15_smoke.rs — fumée E2E M15 : mobile-app ↔ cyberdeck-server
//
//  Vérifie que la lib `cyberdeck_api::rules::remote::fetch_remote_pack` est
//  capable de parler à `cyberdeck-server` en local (port 8080) et de
//  désérialiser correctement l'enveloppe JSON `LatestPackResponse`.
//
//  Usage :
//    cargo run --example m15_smoke -p cyberdeck-api
//    # On force d'abord un cache vide, puis on appelle deux fois :
//    #   1er run  : updated=true  (cache vide)
//    #   2e  run  : updated=false (ETag match → 304 ou SHA identique)
// =============================================================================
use cyberdeck_api::rules::remote::fetch_remote_pack as fetch_rules;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    unsafe {
        std::env::remove_var("CYBERDECK_RULES_PACK_URL");
        std::env::remove_var("WRAITH_RULES_URL");
        std::env::set_var("WRAITH_CACHE_DIR", "/tmp/cyberdeck-m15-cache");
    }
    let _ = std::fs::remove_dir_all("/tmp/cyberdeck-m15-cache");
    // Premier appel : cache vide.
    match fetch_rules(None).await {
        Ok(r) => println!(
            "[1] OK rules: updated={} version={} count={} url={}",
            r.updated, r.pack_version, r.rules_count, r.source_url
        ),
        Err(e) => println!("[1] ERR rules: {e}"),
    }
    // Deuxième appel : cache populé → on attend updated=false (ETag round-trip).
    match fetch_rules(None).await {
        Ok(r) => println!(
            "[2] OK rules: updated={} version={} count={} url={}",
            r.updated, r.pack_version, r.rules_count, r.source_url
        ),
        Err(e) => println!("[2] ERR rules: {e}"),
    }
}
