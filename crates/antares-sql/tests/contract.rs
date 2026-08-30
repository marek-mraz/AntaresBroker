// SPDX-License-Identifier: EUPL-1.2
//! Every backend in this crate is held to the driver contract
//! (`antares_store::contract`), the rules `antares-api` writes against and
//! no backend decides for itself. The Postgres arms run the same two
//! functions in `pg.rs`, where a live server is available.

use antares_model::TenantId;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;

fn tenants() -> (TenantId, TenantId) {
    (
        TenantId::new("contracta").expect("tenant"),
        TenantId::new("contractb").expect("tenant"),
    )
}

#[test]
fn the_memory_store_keeps_the_driver_contract() {
    let (a, b) = tenants();
    let s = AnyStore::Mem(Store::default());
    antares_store::contract::run_current_state_contract(&s, &a, &b, "memory");
    antares_store::contract::run_temporal_contract(&s, &a, &b, "memory");
}

#[test]
fn the_file_store_keeps_the_driver_contract() {
    let (a, b) = tenants();
    let dir = std::env::temp_dir().join(format!("antares-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    let s = AnyStore::Mem(Store::open_file(&dir).expect("open"));
    antares_store::contract::run_current_state_contract(&s, &a, &b, "file");
    antares_store::contract::run_temporal_contract(&s, &a, &b, "file");
    let _ = std::fs::remove_dir_all(&dir);
}
