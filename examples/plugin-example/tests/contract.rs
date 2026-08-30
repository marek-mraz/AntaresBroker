// SPDX-License-Identifier: EUPL-1.2
//! The plugin proves itself with the same function the built-in backends
//! run — that is the point of the contract kit: a driver written outside
//! this workspace is held to exactly the rules `antares-api` writes against,
//! without copying anyone's tests.

use antares_model::TenantId;
use antares_plugin_example::ExampleStore;

#[test]
fn the_example_driver_keeps_the_current_state_contract() {
    let a = TenantId::new("plugina").expect("tenant");
    let b = TenantId::new("pluginb").expect("tenant");
    let store = ExampleStore::new();
    antares_store::contract::run_current_state_contract(&store, &a, &b, "plugin");
}

#[test]
fn the_example_driver_keeps_the_temporal_contract() {
    let a = TenantId::new("plugintempa").expect("tenant");
    let b = TenantId::new("plugintempb").expect("tenant");
    let store = ExampleStore::new();
    antares_store::contract::run_temporal_contract(&store, &a, &b, "plugintemp");
}
