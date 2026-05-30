//! Sprint 1 — live ADT smoke test against a real SAP development system.
//!
//! Gated on `ORACLE_AUTOMATE_DESTINATION`: with no destination configured the
//! test prints a skip notice and passes, so CI (and any contributor without
//! SAP access) stays green.  This mirrors the gated SAP Business Hub sandbox
//! test in `crates/oracle-automate-rfc/src/odata.rs`.
//!
//! ## Running it against a dev Oracle Fusion Cloud ERP system
//!
//! 1. Create `./.oracle-automate/destinations/<name>.toml` — see
//!    `deploy/oracle-automate-destination.example.toml` for the schema.
//! 2. Run:
//!
//! ```bash
//! ORACLE_AUTOMATE_DESTINATION=<name> \
//!   cargo test -p oracle-automate-server --test live_adt -- --nocapture
//! ```
//!
//! Override the probed class with `ORACLE_AUTOMATE_TEST_CLASS` if the default
//! is unavailable on your stack.

use oracle_automate_adt::{HttpOicClient, OicAuth, OicClient, OicDestination};

#[tokio::test]
async fn live_adt_get_class_smoke() {
    let Ok(name) = std::env::var("ORACLE_AUTOMATE_DESTINATION") else {
        eprintln!(
            "SKIP live_adt_get_class_smoke: set ORACLE_AUTOMATE_DESTINATION=<name> \
             (with a destination TOML on the search path) to exercise a real SAP system"
        );
        return;
    };
    if name.is_empty() {
        eprintln!("SKIP live_adt_get_class_smoke: ORACLE_AUTOMATE_DESTINATION is empty");
        return;
    }

    let dest = OicDestination::load(&name)
        .unwrap_or_else(|e| panic!("destination '{name}' load failed: {e}"));
    assert!(
        !matches!(dest.auth, OicAuth::Mock),
        "live test needs a non-mock destination; '{name}' declares auth=mock"
    );

    let client = HttpOicClient::new(dest).expect("HttpOicClient init");

    // A class that exists on essentially every OIC/custom-code stack.
    let class = std::env::var("ORACLE_AUTOMATE_TEST_CLASS")
        .unwrap_or_else(|_| "CL_ABAP_CHAR_UTILITIES".to_string());

    let src = client
        .get_groovy_script(&class)
        .await
        .unwrap_or_else(|e| panic!("get_groovy_script({class}) against '{name}' failed: {e}"));

    assert!(
        !src.source.is_empty(),
        "expected non-empty OIC/custom-code source for {class}"
    );
    eprintln!(
        "live_adt OK: fetched {} from destination '{}' ({} bytes of source)",
        class,
        name,
        src.source.len()
    );
}
