//! Antares — NGSI-LD context broker (composition root).
//!
//! Config: ANTARES_* env vars only for v0 (antares.toml layering can land
//! later via figment). Unknown ANTARES_* keys are fatal.

mod shutdown;
mod telemetry;
mod wiring;

use antares_api::AppState;
use antares_bus::LocalBus;

// ANTARES_DATABASE_URL: accepted — the ETSI compose wires one DB per broker —
// consumed by the postgres/timescale store modes.
const KNOWN_KEYS: &[&str] = &[
    "ANTARES_HTTP_PORT",
    "ANTARES_HOST_ALIAS",
    // 5.8.1.4 distributed subscriptions: the public base URL other brokers
    // reach this one at (the reduced-copy notification endpoint); defaults
    // to http://{host_alias}.
    "ANTARES_PUBLIC_URL",
    "ANTARES_ROLES",
    "ANTARES_DATABASE_URL",
    "ANTARES_STORE",
    // Temporal driver: a store mode, or `none` — history off (temporal
    // reads answer OperationNotSupported, Table 6.3.2-1). Defaults to the
    // current-state store, so one instance serves both seams.
    "ANTARES_TEMPORAL",
    // History gate: `all` (default) records every changed instance;
    // `observed` records only instances carrying observedAt; `none`
    // auto-records nothing (temporal API + reads stay on).
    "ANTARES_TEMPORAL_RECORD",
    "ANTARES_DATA_DIR",
    // Egress: private-range destinations are ALLOWED by default (ADR-0010 —
    // brokers federate inside private networks); a hardened deployment sets
    // this to false to arm the SSRF wall.
    "ANTARES_EGRESS_ALLOW_PRIVATE",
    // Refuse to start when the DB role bypasses RLS (production gate;
    // default off so the dev/ETSI superuser stack still boots).
    "ANTARES_REQUIRE_RLS",
    // Temporal retention horizon in days; absent = keep forever (a
    // maintenance job must never default to dropping data).
    "ANTARES_TEMPORAL_RETENTION_DAYS",
    // 4.22 GC interval (memory/file arm); default 900 s, the ETSI stack runs
    // at 2 s so the transient TPs (422_01) exercise the sweep itself.
    "ANTARES_SWEEP_SECS",
    // Batch entity-count cap; default 1000 — raised where a
    // trusted producer legitimately batches larger (the spec sets no ceiling).
    "ANTARES_MAX_BATCH_ITEMS",
    // Drain: the LB notice window, and the ceiling on waiting for
    // in-flight requests once the listener has closed.
    "ANTARES_DRAIN_DELAY_MS",
    "ANTARES_DRAIN_DEADLINE_SECS",
    // Optional PEM bundle of extra TLS trust anchors (private CAs,
    // incomplete-chain servers). Never disables verification.
    "ANTARES_EXTRA_CA_FILE",
    // The bus seam: local (default, single process, all roles) or
    // nats (the JetStream spine — requires a postgres/timescale store and
    // ANTARES_NATS_URL).
    "ANTARES_BUS",
    "ANTARES_NATS_URL",
    // Stream/KV replication factor on a clustered JetStream (3 for
    // the reference manifests' R3; default 1 for single-node).
    "ANTARES_NATS_REPLICAS",
    // OTLP/HTTP span export endpoint (e.g. http://collector:4318/v1/traces);
    // unset = no OTLP anywhere.
    "ANTARES_OTLP_ENDPOINT",
    "ANTARES_TELEMETRY",
    // Outbox drain on this pod, on (default) | off. `off` is the
    // crash-drill lever (rows commit but this pod never publishes them —
    // another pod's drain must) and the knob for a dedicated-drainer split.
    "ANTARES_OUTBOX_DRAIN",
    // Notification delivery policy: total attempts (default 1 = 5.8.6 as
    // written), first-retry backoff, and the age after which no retry
    // starts. An exhausted policy leaves a dead letter (/q/dead-letters).
    "ANTARES_NOTIFY_ATTEMPTS",
    "ANTARES_NOTIFY_BACKOFF_MS",
    "ANTARES_NOTIFY_MAX_AGE_SECS",
    // Postgres pool size (max connections); default 20.
    "ANTARES_PG_POOL",
    "ANTARES_PG_STATEMENT_TIMEOUT_MS",
    // bus=local over a shared postgres/timescale store is refused — every
    // replica would run its own matcher and fire its own copy of each
    // notification. This opt-in states the deployment runs exactly ONE
    // broker process against that database.
    "ANTARES_ALLOW_SHARED_LOCAL",
    // HTTP/1 header read timeout in ms (default 10000): a connection that
    // never finishes its request headers is closed instead of holding a
    // slot forever.
    "ANTARES_HEADER_READ_TIMEOUT_MS",
    // Ceiling on concurrently served connections (default 10000);
    // connections accepted above it are dropped immediately.
    "ANTARES_MAX_CONNECTIONS",
    // 5.7.2.4 fan-out ceiling: how many matching registrations one
    // distributed operation may contact.
    "ANTARES_FED_FANOUT",
    // Ceiling on the body this broker will read back from a forwarded
    // request.
    "ANTARES_MAX_FED_RESPONSE_BYTES",
    // 5.7.5/5.7.6 discovery scan ceiling (types/attributes listing).
    "ANTARES_DISCOVERY_SCAN_MAX",
    // Run the DDL on this process; off keeps replicas from racing the
    // migration on boot.
    "ANTARES_MIGRATE",
];

/// Jemalloc with decay-based purging — RSS returns to ~live×1.2 when idle;
/// tune via MALLOC_CONF (e.g. dirty_decay_ms). Deliberately not glibc
/// malloc, whose arena fragmentation never gives memory back.
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// Unknown-config-is-fatal, minus what the platform injects: a Service
/// named `antares*` makes kubelet write ANTARES_PORT,
/// ANTARES_PORT_9090_TCP*, ANTARES_SERVICE_* (and the antares-file /
/// antares-api variants) into every pod, and treating those as typos put the
/// shipped manifests into 100% CrashLoopBackOff. The manifests also set
/// enableServiceLinks: false — this check is the belt for clusters that
/// re-enable links or add their own Services.
fn unknown_config_key(key: &str) -> bool {
    if !key.starts_with("ANTARES_") || key.starts_with("ANTARES_TEST_") {
        return false;
    }
    if KNOWN_KEYS.contains(&key) {
        return false;
    }
    // kubelet service-link shapes for the Services OUR manifests ship
    // (antares, antares-file, antares-api, antares-worker): {NAME}_PORT,
    // {NAME}_PORT_<n>_<proto>*, {NAME}_SERVICE_*. Only those exact name
    // infixes are exempt — an arbitrary ANTARES_*-shaped var stays a fatal
    // typo, and foreign Services are covered by
    // enableServiceLinks: false in the manifests.
    let rest = &key["ANTARES_".len()..];
    let injected = ["", "FILE_", "API_", "WORKER_"].iter().any(|infix| {
        rest.strip_prefix(infix)
            .is_some_and(|t| t == "PORT" || t.starts_with("PORT_") || t.starts_with("SERVICE_"))
    });
    !injected
}

/// ANTARES_SWEEP_SECS paces the 4.22 expiry sweep in every store mode. Absent
/// is the 15 min default; anything that is not a positive integer is fatal,
/// because a garbage cadence silently becoming the default one is exactly the
/// misconfiguration the unknown-key policy exists to catch.
fn parse_sweep_secs(raw: Option<&str>) -> Result<u64, String> {
    let Some(v) = raw else {
        return Ok(15 * 60);
    };
    match v.parse::<u64>() {
        Ok(0) | Err(_) => Err(format!(
            "ANTARES_SWEEP_SECS must be a positive integer number of seconds, got {v:?}"
        )),
        Ok(n) => Ok(n),
    }
}

/// An explicitly-off switch value, whatever the operator's spelling. Used by
/// the knobs where the DEFAULT is off, so that only an off value keeps them
/// off and a typo cannot silently disable a security control.
fn is_off(v: &str) -> bool {
    let v = v.trim();
    v.is_empty()
        || v == "0"
        || v.eq_ignore_ascii_case("false")
        || v.eq_ignore_ascii_case("off")
        || v.eq_ignore_ascii_case("no")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --version answers without starting anything (a bare `antares
    // --version` used to boot a server).
    // `args()`/`vars()` PANIC on non-UTF-8; the *_os variants do not, and a
    // stray byte in the environment or argv must not kill the process.
    if std::env::args_os().any(|a| a == "--version" || a == "-V") {
        println!(
            "antares {} ({})",
            env!("CARGO_PKG_VERSION"),
            antares_api::GIT_HASH
        );
        return Ok(());
    }
    // Tracing (fmt + env-gated OTLP [+ console feature]) and, with the
    // `telemetry` feature, the Prometheus recorder rendering /q/metrics.
    let metrics_render = telemetry::init()?;

    // Unknown-config-is-fatal: catch typos before they become Scorpio's
    // $[quarkus.uuid} class of silent misconfiguration. ANTARES_TEST_* is the
    // reserved harness namespace (ANTARES_TEST_DATABASE_URL, ANTARES_TEST_MQTT_URL,
    // …) — CI exports those for the integration tests, and they land in the env of
    // any broker a test spawns. Reserving the prefix here beats making every
    // spawn site remember an env_remove allowlist.
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if unknown_config_key(&key) {
            return Err(format!("unknown config key {key} (known: {KNOWN_KEYS:?})").into());
        }
    }

    let port_raw = std::env::var("ANTARES_HTTP_PORT").unwrap_or_else(|_| "9090".into());
    let port: u16 = port_raw.parse().map_err(|e| {
        format!("ANTARES_HTTP_PORT must be a port number 0-65535, got {port_raw:?} ({e})")
    })?;
    // Every remaining config value is parsed HERE, before the runtime starts,
    // so a garbage window, cadence or switch fails the process instead of
    // silently running at its default.
    let sweep_secs = parse_sweep_secs(std::env::var("ANTARES_SWEEP_SECS").ok().as_deref())?;
    // Same: validated here, read again where the state is built.
    antares_api::DeliveryPolicy::from_env()?;
    let drain_delay = shutdown::drain_delay()?;
    let drain_deadline = shutdown::drain_deadline()?;
    // Validated here so a typo fails startup; the value itself is read again
    // where the drain task is wired, which is the only place it is used.
    wiring::outbox_drain_enabled()?;
    let host_alias = std::env::var("ANTARES_HOST_ALIAS").unwrap_or_else(|_| "antares".into());
    // 6.3.18 sends this as the Via pseudonym — an RFC 7230 token. `~` is
    // reserved as the tenant separator (federation::alias_for), so allowing
    // it in the configured alias would let `a~b` in the default tenant
    // collide with `a` in tenant `b` and cross-detect as a loop. Fatal at
    // startup, like every other bad config value.
    if host_alias.is_empty()
        || !host_alias
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|".contains(&b))
    {
        return Err(format!(
            "ANTARES_HOST_ALIAS {host_alias:?} is not a valid RFC 7230 token \
             (and may not contain '~', the tenant separator)"
        )
        .into());
    }
    let roles = std::env::var("ANTARES_ROLES").unwrap_or_else(|_| "all".into());
    // Unknown store mode is fatal BEFORE the runtime spins up. The mode
    // is decided ONCE here as a typed value and threaded everywhere — no
    // string comparisons, no runtime re-probing downstream.
    let mode: antares_sql::StoreMode = std::env::var("ANTARES_STORE")
        .unwrap_or_else(|_| "memory".into())
        .parse()
        .map_err(|e| format!("unknown ANTARES_STORE: {e}"))?;
    // bus=local wires an in-process matcher into every process, so N
    // replicas over ONE shared database each fire their own copy of every
    // notification. Refused here — before any store connection is attempted
    // — unless the deployment states it runs exactly one broker process.
    // (Mirror of the nats arm's store check in `run`.)
    let bus_mode = std::env::var("ANTARES_BUS").unwrap_or_else(|_| "local".into());
    if bus_mode == "local"
        && mode.is_pg()
        && !std::env::var("ANTARES_ALLOW_SHARED_LOCAL")
            .is_ok_and(|v| matches!(v.as_str(), "1" | "true"))
    {
        return Err(format!(
            "ANTARES_BUS=local with ANTARES_STORE={mode} double-fires notifications when \
             more than one broker process shares the database (each process runs its own \
             matcher). Use ANTARES_BUS=nats, or set ANTARES_ALLOW_SHARED_LOCAL=1 for a \
             strictly single-process deployment"
        )
        .into());
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let (store, temporal, backend) = build_drivers(mode).await?;
            run(
                port,
                host_alias,
                roles,
                store,
                temporal,
                mode,
                backend,
                metrics_render,
                sweep_secs,
                drain_delay,
                drain_deadline,
            )
            .await
        })
}

/// ANTARES_PG_STATEMENT_TIMEOUT_MS → the per-session `statement_timeout`
/// every pooled connection carries (a runaway query is cancelled, 5.5.2
/// InternalError); absent = 30 000; not a positive integer = fatal.
fn parse_pg_statement_timeout(raw: Option<&str>) -> Result<std::time::Duration, String> {
    match raw {
        None => Ok(std::time::Duration::from_secs(30)),
        Some(v) => match v.parse::<u64>() {
            Ok(n) if n > 0 => Ok(std::time::Duration::from_millis(n)),
            _ => Err(format!(
                "ANTARES_PG_STATEMENT_TIMEOUT_MS must be a positive integer (got {v:?})"
            )),
        },
    }
}

/// ANTARES_PG_POOL → pool size: absent defaults to 20; anything that is not
/// a positive integer is fatal (a misread size must never silently run with
/// a default, matching the unknown-key policy).
fn parse_pg_pool(raw: Option<&str>) -> Result<u32, String> {
    match raw {
        None => Ok(20),
        Some(v) => match v.parse::<u32>() {
            Ok(n) if n > 0 => Ok(n),
            _ => Err(format!(
                "ANTARES_PG_POOL must be a positive integer (got {v:?})"
            )),
        },
    }
}

/// What ANTARES_TEMPORAL resolved to, before anything is built.
#[derive(Debug, PartialEq, Eq)]
enum TemporalChoice {
    /// The current-state store records and serves history too (default).
    SameAsStore,
    /// History off: `NoTemporal`.
    None,
    /// A second store instance of this mode, used only through its
    /// temporal half.
    Second(antares_sql::StoreMode),
}

/// The shelf this binary was built with — every store mode is compiled in,
/// so the listing is static; a feature-gated backend would drop out here.
const BUILT_WITH: &str = "memory|file|postgres|timescale (temporal also: none)";

/// ANTARES_TEMPORAL → driver choice. Absent or the store's own mode = one
/// instance for both seams; `none` = no history; any other backend name =
/// a second store. An unknown name is fatal and names the shelf.
fn temporal_choice(
    store_mode: antares_sql::StoreMode,
    raw: Option<&str>,
) -> Result<TemporalChoice, String> {
    match raw {
        None => Ok(TemporalChoice::SameAsStore),
        Some(m) if m == store_mode.as_str() => Ok(TemporalChoice::SameAsStore),
        Some("none") => Ok(TemporalChoice::None),
        Some(other) => other.parse().map(TemporalChoice::Second).map_err(|_| {
            format!("ANTARES_TEMPORAL: unknown backend {other:?}; built with {BUILT_WITH}")
        }),
    }
}

/// The backend registry: the two driver seams from their configured names.
/// Every backend is one arm of `build_store`; the temporal driver is by
/// default the same instance (history recorded and served by the
/// current-state store), `none` turns history off (temporal reads answer
/// OperationNotSupported 422, Table 6.3.2-1; the recorder produces
/// nothing), and a different backend name builds a second store used only
/// through its temporal half.
async fn build_drivers(
    store_mode: antares_sql::StoreMode,
) -> Result<
    (
        std::sync::Arc<antares_sql::store::any::AnyStore>,
        std::sync::Arc<dyn antares_store::TemporalDriver>,
        Option<antares_sql::maintenance::TemporalBackend>,
    ),
    Box<dyn std::error::Error>,
> {
    let (store, backend) = build_store(store_mode).await?;
    let store = std::sync::Arc::new(store);
    let raw = std::env::var("ANTARES_TEMPORAL").ok();
    let temporal: std::sync::Arc<dyn antares_store::TemporalDriver> =
        match temporal_choice(store_mode, raw.as_deref())? {
            TemporalChoice::SameAsStore => store.clone(),
            TemporalChoice::None => std::sync::Arc::new(antares_store::NoTemporal),
            TemporalChoice::Second(mode) => {
                let (second, _) = build_store(mode).await?;
                std::sync::Arc::new(second)
            }
        };
    Ok((store, temporal, backend))
}

/// ANTARES_STORE → store construction: `file` requires ANTARES_DATA_DIR
/// (never a default inside the image); postgres and timescale require
/// ANTARES_DATABASE_URL, connect ONE shared pool, run the embedded
/// migrations at start and serve from the Pg backend.
async fn build_store(
    mode: antares_sql::StoreMode,
) -> Result<
    (
        antares_sql::store::any::AnyStore,
        Option<antares_sql::maintenance::TemporalBackend>,
    ),
    Box<dyn std::error::Error>,
> {
    use antares_sql::maintenance::TemporalBackend;
    use antares_sql::store::any::{AnyStore, PgBackend};
    use antares_sql::store::Store;
    use antares_sql::StoreMode;
    match mode {
        StoreMode::Memory => Ok((AnyStore::Mem(Store::default()), None)),
        StoreMode::File => {
            let dir = std::env::var("ANTARES_DATA_DIR").map_err(|_| {
                "ANTARES_STORE=file requires ANTARES_DATA_DIR (a mounted volume — data \
                 must never live inside the image)"
            })?;
            let dir = std::path::PathBuf::from(dir);
            warn_if_not_mount_point(&dir);
            Ok((AnyStore::Mem(Store::open_file(&dir)?), None))
        }
        StoreMode::Postgres | StoreMode::Timescale => {
            let url = std::env::var("ANTARES_DATABASE_URL")
                .map_err(|_| format!("ANTARES_STORE={mode} requires ANTARES_DATABASE_URL"))?;
            let pool_size = parse_pg_pool(std::env::var("ANTARES_PG_POOL").ok().as_deref())?;
            let statement_timeout = parse_pg_statement_timeout(
                std::env::var("ANTARES_PG_STATEMENT_TIMEOUT_MS")
                    .ok()
                    .as_deref(),
            )?;
            // The DB container may still be booting — bounded retry, then die.
            let mut last = String::new();
            for _ in 0..30 {
                match antares_sql::pg::connect_with(&url, pool_size, statement_timeout).await {
                    Ok(pool) => {
                        // The temporal backend is what the migrations actually
                        // BUILT, detected once from the catalog and pinned —
                        // the maintenance branch can never disagree with the
                        // DDL on disk, whatever happened to the extension since.
                        let backend = antares_sql::maintenance::detect_temporal_backend(&pool)
                            .await
                            .map_err(|e| format!("ANTARES_STORE={mode}: {e}"))?;
                        // Never silently fall back — timescale mode whose
                        // database is not hypertable-shaped is a config error,
                        // not a downgrade (extension missing at first boot, or
                        // installed only after the migrations ran).
                        if mode == StoreMode::Timescale && backend != TemporalBackend::Hypertable {
                            return Err(format!(
                                "ANTARES_STORE=timescale but attr_instances is {backend:?} — \
                                 the timescaledb extension was not CREATEd when the migrations \
                                 first ran. Install it in a fresh database (CREATE EXTENSION \
                                 timescaledb before first boot) or use ANTARES_STORE=postgres"
                            )
                            .into());
                        }
                        if mode == StoreMode::Postgres && backend == TemporalBackend::Hypertable {
                            tracing::info!(
                                "attr_instances is a hypertable (migrations ran with \
                                 the timescaledb extension present); the plain-mode partition \
                                 job stands down, retention runs via drop_chunks"
                            );
                        }
                        // RLS is a belt only when the role wears it —
                        // superuser/BYPASSRLS makes every policy inert. Warn
                        // always; in production set ANTARES_REQUIRE_RLS=1 to turn
                        // the warning into a hard refusal so a superuser DSN can
                        // never silently ship (dev/ETSI stacks leave it unset).
                        if antares_sql::pg::role_bypasses_rls(&pool).await {
                            // A gate that only understands two spellings
                            // fails OPEN on `TRUE`/`yes`/`on`: the operator
                            // believes RLS is enforced and the broker serves
                            // with a BYPASSRLS role. Anything but an explicit
                            // off value turns it on.
                            let strict =
                                std::env::var("ANTARES_REQUIRE_RLS").is_ok_and(|v| !is_off(&v));
                            if strict {
                                return Err(
                                    "ANTARES_REQUIRE_RLS=1 but the database role bypasses \
                                     row-level security (superuser or BYPASSRLS) — connect as a \
                                     non-superuser, non-BYPASSRLS role so the RLS tenant-isolation \
                                     backstop is enforced"
                                        .into(),
                                );
                            }
                            tracing::warn!(
                                "database role bypasses row-level security (superuser or \
                                 BYPASSRLS) — tenant isolation rests on the explicit \
                                 predicates only; use a non-superuser role in production \
                                 (set ANTARES_REQUIRE_RLS=1 to enforce)"
                            );
                        }
                        tracing::info!(
                            "ANTARES_STORE={mode}: pool up, migrations applied, serving \
                             from postgres (temporal backend: {backend:?})"
                        );
                        return Ok((AnyStore::Pg(PgBackend::new(pool)), Some(backend)));
                    }
                    Err(e) => {
                        last = e.to_string();
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
            Err(format!("ANTARES_STORE={mode}: database not reachable after 30 s: {last}").into())
        }
    }
}

/// Warn when the data dir shares a device with its parent — i.e. it is
/// not a mount point, so the redb file dies with the container.
#[cfg(unix)]
fn warn_if_not_mount_point(dir: &std::path::Path) {
    use std::os::unix::fs::MetadataExt;
    let _ = std::fs::create_dir_all(dir);
    if let (Ok(md), Some(Ok(parent_md))) =
        (std::fs::metadata(dir), dir.parent().map(std::fs::metadata))
    {
        if md.dev() == parent_md.dev() {
            eprintln!(
                "WARN: ANTARES_DATA_DIR {} is not a mount point — data will be lost when \
                 the container is removed",
                dir.display()
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_not_mount_point(_dir: &std::path::Path) {}

// Every parameter is one config value `main` parsed fatally before the
// runtime started; a struct would only rename the same ten values.
#[allow(clippy::too_many_arguments)]
async fn run(
    port: u16,
    host_alias: String,
    roles: String,
    // the concrete handle stays for the backend-specific jobs; the
    // AppState carries only the driver seam
    store: std::sync::Arc<antares_sql::store::any::AnyStore>,
    temporal: std::sync::Arc<dyn antares_store::TemporalDriver>,
    store_mode: antares_sql::StoreMode,
    temporal_backend: Option<antares_sql::maintenance::TemporalBackend>,
    metrics_render: Option<telemetry::MetricsRender>,
    sweep_secs: u64,
    drain_delay: std::time::Duration,
    drain_deadline: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let _bus = LocalBus::new(1024); // local-mode ring (in-process hook path)
    let roles = wiring::Roles::parse(&roles).map_err(|e| format!("ANTARES_ROLES: {e}"))?;
    // Bus seam: local (default) or nats. An unknown value is fatal.
    let bus_mode = std::env::var("ANTARES_BUS").unwrap_or_else(|_| "local".into());
    match bus_mode.as_str() {
        "local" => {
            // bus=local means ONE process running every role — a role
            // split without a shared bus would silently drop whole concerns.
            if !roles.all() {
                return Err(
                    "ANTARES_BUS=local requires all roles in one process (ANTARES_ROLES=all); \
                     role splits need ANTARES_BUS=nats"
                        .into(),
                );
            }
        }
        "nats" => {
            if !store_mode.is_pg() {
                return Err(format!(
                    "ANTARES_BUS=nats requires a shared store (ANTARES_STORE=postgres|timescale); \
                     {store_mode} state is per-process and cannot back multiple instances"
                )
                .into());
            }
        }
        other => return Err(format!("unknown ANTARES_BUS={other} (local|nats)").into()),
    }
    tracing::info!(port, %store_mode, %bus_mode, ?roles, "starting antares");

    // Trailing-slash tolerance: Table 6.2-1 spells collection resources with a
    // trailing '/'; normalize before routing.
    let mut state = AppState::with_drivers(host_alias, store.clone(), temporal, store_mode);
    state.delivery = antares_api::DeliveryPolicy::from_env().unwrap_or_default();
    state.temporal_record = std::env::var("ANTARES_TEMPORAL_RECORD")
        .as_deref()
        .unwrap_or("all")
        .parse()?;
    // /q/metrics renders through this closure (None without the
    // `telemetry` feature — the endpoint answers 404); the sampler feeds
    // the process-level gauges the whole run.
    state.metrics_render = metrics_render;
    // Heap stats on /q/health (allocated/resident bytes via jemalloc-ctl)
    state.mem_stats = Some(std::sync::Arc::new(|| {
        use tikv_jemalloc_ctl::{epoch, stats};
        let _ = epoch::advance();
        serde_json::json!({
            "allocatedBytes": stats::allocated::read().unwrap_or(0),
            "residentBytes": stats::resident::read().unwrap_or(0),
        })
    }));
    if bus_mode == "nats" {
        // Outbox producer + drain, KV/registry mirrors, durable
        // consumers per role, topology asserted before traffic.
        let url = std::env::var("ANTARES_NATS_URL")
            .map_err(|_| "ANTARES_BUS=nats requires ANTARES_NATS_URL")?;
        wiring::wire_nats(&mut state, &url, roles).await?;
    } else {
        antares_api::notify::wire(&mut state); // in-process matcher + notifier + interval firing
    }
    telemetry::spawn_sampler(state.clone());

    // Boot preload — Cached rows persisted by the AppState write-through
    // re-seed the parsed-context cache on start, so expansion doesn't refetch
    // what a previous life already downloaded. (The writer itself is wired in
    // AppState::with_store — rows are the 5.13 source of truth.)
    {
        for row in state.store.context_list().unwrap_or_default() {
            if row.get("kind").and_then(|v| v.as_str()) != Some("Cached") {
                continue;
            }
            let (Some(url), Some(id), Some(created)) = (
                row.get("url").and_then(|v| v.as_str()),
                row.get("localId").and_then(|v| v.as_str()),
                row.get("createdAt").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            if let Some(v) = row.pointer("/body/@context") {
                state.loader.seed_cached(url, id, created, v.clone()).await;
            }
        }
    }

    // 4.22 GC on the memory/file arm: reads already refuse expired entities;
    // this reaps them (spec-sanctioned lag). The Pg arm's sweep runs inside
    // the maintenance job below — one job per backend, mode-switched.
    // ANTARES_SWEEP_SECS paces 4.22 GC identically across ALL store modes —
    // the Mem/file sweep loop and the Pg/Timescale maintenance job below both
    // tick on it (the ETSI stack runs at 2 s so transient TPs observe GC, not
    // just the read filter); parsed at startup, default 15 min.
    if matches!(store.as_ref(), antares_sql::store::any::AnyStore::Mem(_)) {
        let store = state.store.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(sweep_secs));
            loop {
                tick.tick().await;
                let n = store.sweep_expired();
                if n > 0 {
                    tracing::debug!("4.22 sweep reaped {n} expired entities");
                }
            }
        });
    }
    // Temporal maintenance — plain-mode partition pre-creation and the
    // (opt-in) retention horizon, single-winner via SKIP LOCKED.
    // The branch is PINNED to the detected backend (never re-probed): memory
    // and file modes have no backend and get no job, for sure.
    if let (antares_sql::store::any::AnyStore::Pg(p), Some(backend)) =
        (store.as_ref(), temporal_backend)
    {
        let pool = p.docs.pool().clone();
        let retention: Option<i64> = std::env::var("ANTARES_TEMPORAL_RETENTION_DAYS")
            .ok()
            .map(|v| {
                v.parse::<i64>()
                    .map_err(|_| "ANTARES_TEMPORAL_RETENTION_DAYS must be an integer")
                    // A zero/negative horizon inverts `now() - make_interval(days)`
                    // and would reap all current + future history — a data-loss
                    // footgun. Retention is opt-in; a bad value must not silently
                    // delete. Cap at i32 too (bound as $1::int downstream).
                    .and_then(|d| {
                        (d > 0 && d <= i64::from(i32::MAX)).then_some(d).ok_or(
                            "ANTARES_TEMPORAL_RETENTION_DAYS must be between 1 and 2147483647",
                        )
                    })
            })
            .transpose()?;
        tokio::spawn(async move {
            // Same ANTARES_SWEEP_SECS cadence as the Mem arm — the job's 4.22
            // reap is the sweep here; the partition/retention steps riding on
            // the same tick are idempotent and SKIP LOCKED single-winner.
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(sweep_secs));
            loop {
                tick.tick().await; // first tick is immediate: partitions at boot
                match antares_sql::maintenance::temporal_maintenance(&pool, backend, retention)
                    .await
                {
                    Ok(msg) => tracing::debug!("temporal maintenance: {msg}"),
                    Err(e) => tracing::warn!("temporal maintenance failed: {e}"),
                }
            }
        });
    }
    // Handles the drain needs, taken before `state` is consumed by the
    // router — the flag the health endpoint reads, and the store whose pools
    // close last.
    let draining = state.draining.clone();
    let store_for_drain = state.store.clone();
    // Only the api role serves the NGSI-LD surface — a worker pod
    // exposes health/ready/metrics and nothing else (a subscription created
    // on a worker would bypass the roles.api KV sync and never notify).
    let routed = if roles.api {
        antares_api::router(state)
    } else {
        antares_api::ops_router(state)
    };
    let app = tower::Layer::layer(
        &tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash(),
        routed,
    );

    // A connection that never finishes its request headers must not hold a
    // slot forever; hyper closes it after this timeout.
    let header_read_timeout = std::env::var("ANTARES_HEADER_READ_TIMEOUT_MS")
        .ok()
        .map(|v| {
            v.parse::<u64>().map_err(|_| {
                format!("ANTARES_HEADER_READ_TIMEOUT_MS must be an integer (got {v:?})")
            })
        })
        .transpose()?
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(10));
    // Ceiling on concurrently served connections: accepted streams beyond
    // it are dropped at once — refusing cheaply beats queueing work the box
    // cannot serve (each served connection is a spawned task).
    let max_connections = std::env::var("ANTARES_MAX_CONNECTIONS")
        .ok()
        .map(|v| {
            v.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| {
                format!("ANTARES_MAX_CONNECTIONS must be a positive integer (got {v:?})")
            })
        })
        .transpose()?
        .unwrap_or(10_000);
    let conn_permits = std::sync::Arc::new(tokio::sync::Semaphore::new(max_connections));

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on http://0.0.0.0:{port}");
    // Count open connections so the drain can wait for them. Incremented
    // before the task is spawned — incrementing inside the task would race the
    // drain's first check and let a just-accepted connection be missed.
    // This counts CONNECTIONS, not requests, and stays that way on purpose:
    // hyper's graceful_shutdown below draws the distinction already (an idle
    // keep-alive closes at once, an active request finishes first), so a
    // request-layer counter would add a middleware without changing what the
    // drain waits for.
    let inflight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // The drain signal each connection listens for. On drain,
    // hyper's graceful_shutdown closes IDLE keep-alive connections immediately
    // (the LB holds one per backend — counting them as in-flight made every
    // api roll burn the full deadline) while an active request still finishes.
    let (drain_tx, drain_rx) = tokio::sync::watch::channel(false);
    // The signal future is created ONCE and polled by reference. Written
    // inline in the select, it would be dropped and re-created on every
    // accepted connection — and a SIGTERM landing in that drop-to-recreate
    // window is lost for good (tokio signal streams do not replay events from
    // before their creation). Under health-check polling that window is hit
    // constantly, which is exactly how the drain test caught it.
    let mut sigterm = std::pin::pin!(shutdown::signal());
    // A pod whose drain is switched off publishes nothing, so waiting for the
    // outbox to empty there would only burn the deadline.
    let flush_outbox = wiring::outbox_drain_enabled()?;
    // Manual serve loop: the ETSI suite reads response headers case-sensitively
    // ("Location"), so HTTP/1 responses are written with title-case headers.
    loop {
        let stream = tokio::select! {
            s = accept(&listener) => s,
            _ = &mut sigterm => {
                // 1+2: unhealthy FIRST, then keep serving for the LB's notice
                // window — still inside this select, so connections arriving
                // during it are accepted normally.
                shutdown::begin(&draining, drain_delay);
                let until = tokio::time::Instant::now() + drain_delay;
                loop {
                    tokio::select! {
                        stream = accept(&listener) => {
                            serve(stream, app.clone(), inflight.clone(), drain_rx.clone(),
                                  conn_permits.clone(), header_read_timeout);
                        }
                        _ = tokio::time::sleep_until(until) => break,
                    }
                }
                // 3–6: listener dropped, idle conns told to close (active
                // requests finish), in-flight drained, pools closed.
                drop(listener);
                let _ = drain_tx.send(true);
                shutdown::drain(&inflight, &*store_for_drain, drain_deadline, flush_outbox).await;
                tracing::info!("shutting down");
                return Ok(());
            }
        };
        serve(
            stream,
            app.clone(),
            inflight.clone(),
            drain_rx.clone(),
            conn_permits.clone(),
            header_read_timeout,
        );
    }
}

/// Accept the next connection. There is no failure value: `accept()`
/// propagates every non-`WouldBlock` errno from the syscall, and an
/// `ECONNABORTED` (a client resetting between SYN and accept), `EMFILE`/
/// `ENFILE` (the fd ceiling — the connection cap defaults above many
/// containers' `nofile`) or `ENOBUFS` must never take the broker down.
async fn accept(listener: &tokio::net::TcpListener) -> tokio::net::TcpStream {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => return stream,
            Err(e) => {
                tracing::warn!("accept failed, retrying: {e}");
                if accept_backoff(&e) {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }
}

/// Whether an `accept()` error warrants a pause before the next attempt.
/// The per-connection failures retry at once (the next connection is
/// unaffected); a resource exhaustion would otherwise spin the loop at full
/// speed until the pressure clears. Neither is fatal.
fn accept_backoff(e: &std::io::Error) -> bool {
    !matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::Interrupted
    )
}

/// The served app: the router under trailing-slash normalization.
type App = tower_http::normalize_path::NormalizePath<axum::Router>;

/// One accepted connection. Split out of the accept loop so the drain's
/// notice window serves connections with identical behaviour, and so the
/// in-flight counter is incremented in exactly one place.
fn serve(
    stream: tokio::net::TcpStream,
    app: App,
    inflight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    mut drain_rx: tokio::sync::watch::Receiver<bool>,
    conn_permits: std::sync::Arc<tokio::sync::Semaphore>,
    header_read_timeout: std::time::Duration,
) {
    // Over the connection cap: drop the accepted stream immediately. The
    // permit rides in the connection task, so the slot frees exactly when
    // the connection ends — and never enters the inflight drain accounting.
    let Ok(permit) = conn_permits.try_acquire_owned() else {
        return;
    };
    inflight.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tokio::spawn(async move {
        let _permit = permit;
        let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
            let mut app = app.clone();
            async move { tower::Service::call(&mut app, req.map(axum::body::Body::new)).await }
        });
        let mut builder =
            hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
        builder
            .http1()
            .timer(hyper_util::rt::TokioTimer::new())
            .header_read_timeout(header_read_timeout)
            .title_case_headers(true);
        let conn = builder.serve_connection(hyper_util::rt::TokioIo::new(stream), svc);
        let mut conn = std::pin::pin!(conn);
        // On drain, close an IDLE keep-alive connection immediately —
        // hyper finishes any active request first, then closes. Without this
        // the LB's idle keep-alives count as in-flight and every roll waits
        // out the entire drain deadline.
        tokio::select! {
            r = conn.as_mut() => { let _ = r; }
            _ = wait_drain(&mut drain_rx) => {
                conn.as_mut().graceful_shutdown();
                let _ = conn.as_mut().await;
            }
        }
        inflight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    });
}

/// Resolves when the drain signal fires; pends forever once the sender is
/// gone (the connection future then completes on its own in the select).
async fn wait_drain(rx: &mut tokio::sync::watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod config_key_tests {
    use super::unknown_config_key;

    /// Kubelet-injected service links must never be fatal; real typos
    /// must stay fatal.
    #[test]
    fn kubelet_service_links_are_not_typos() {
        for k in [
            "ANTARES_PORT",
            "ANTARES_PORT_9090_TCP",
            "ANTARES_PORT_9090_TCP_ADDR",
            "ANTARES_SERVICE_HOST",
            "ANTARES_SERVICE_PORT",
            "ANTARES_API_SERVICE_HOST",
            "ANTARES_FILE_PORT_9090_TCP_PROTO",
        ] {
            assert!(!unknown_config_key(k), "{k} is platform-injected");
        }
        for k in [
            "ANTARES_HTTP_PORT",
            "ANTARES_STORE",
            "ANTARES_TEST_ANYTHING",
            "ANTARES_PG_POOL",
            "ANTARES_ALLOW_SHARED_LOCAL",
            "ANTARES_HEADER_READ_TIMEOUT_MS",
            "ANTARES_MAX_CONNECTIONS",
        ] {
            assert!(!unknown_config_key(k), "{k} is known/reserved");
        }
        assert!(
            unknown_config_key("ANTARES_STROE"),
            "a real typo stays fatal"
        );
        assert!(
            unknown_config_key("ANTARES_HTPT_PORT"),
            "a typo'd *_PORT var is NOT a service link"
        );
        assert!(unknown_config_key("ANTARES_BOGUS_FLAG"));
    }

    /// The exemption is narrow on purpose: only the exact kubelet shapes for
    /// the Services this repo ships. Near-misses stay fatal, and a non-ANTARES
    /// variable is never our business.
    #[test]
    fn the_service_link_exemption_does_not_over_reach() {
        for k in [
            "ANTARES_",
            "ANTARES_PORTAL",
            "ANTARES_SERVICEHOST",
            "ANTARES_DB_PORT",
            "ANTARES_WORKER_PROT",
        ] {
            assert!(unknown_config_key(k), "{k} must stay a fatal typo");
        }
        for k in ["PATH", "HOME", "antares_store", "ANTARE_STORE", ""] {
            assert!(!unknown_config_key(k), "{k} is not broker config");
        }
    }

    /// Unknown keys are FATAL, so every ANTARES_* variable the workspace
    /// actually reads has to be accepted — otherwise setting a documented
    /// deployment knob is a CrashLoopBackOff and the knob cannot be used at
    /// all. Scans the sources rather than restating the list, so the check
    /// keeps holding as crates add knobs.
    #[test]
    fn known_keys_cover_every_variable_the_workspace_reads() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates dir")
            .to_path_buf();
        let needles = [
            concat!("var(", "\"ANTARES_"),
            concat!("var_os(", "\"ANTARES_"),
        ];
        let mut missing: Vec<String> = Vec::new();
        let mut stack = vec![crates];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read_dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).unwrap_or_default();
                for needle in needles {
                    for (i, _) in src.match_indices(needle) {
                        let rest = &src[i + needle.len() - "\"ANTARES_".len() + 1..];
                        let Some(end) = rest.find('"') else { continue };
                        let key = &rest[..end];
                        if unknown_config_key(key) {
                            missing.push(format!("{key} (read in {})", path.display()));
                        }
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "KNOWN_KEYS is missing variables the workspace reads: {missing:#?}"
        );
    }
}

#[cfg(test)]
mod accept_loop_tests {
    use super::{accept, accept_backoff};

    /// An accept() error must never end the serve loop: tokio propagates
    /// every non-WouldBlock errno straight from the syscall, so ECONNABORTED
    /// (a client resetting between SYN and accept), EMFILE/ENFILE (the fd
    /// ceiling) and ENOBUFS all used to take the whole broker down. `accept`
    /// has no failure value at all — the only decision left is whether to
    /// pause before retrying.
    #[test]
    fn no_accept_error_is_fatal_and_resource_errors_back_off() {
        use std::io::ErrorKind::*;
        for kind in [ConnectionAborted, ConnectionReset, Interrupted] {
            assert!(
                !accept_backoff(&std::io::Error::new(kind, "x")),
                "{kind:?} is per-connection — the next accept must run at once"
            );
        }
        for kind in [
            Other,
            OutOfMemory,
            PermissionDenied,
            InvalidInput,
            NotConnected,
        ] {
            assert!(
                accept_backoff(&std::io::Error::new(kind, "x")),
                "{kind:?} would spin the loop without a pause"
            );
        }
    }

    /// …and the healthy path still hands the loop its connection.
    #[tokio::test]
    async fn accept_yields_the_next_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = tokio::spawn(async move { tokio::net::TcpStream::connect(addr).await });
        let stream = tokio::time::timeout(std::time::Duration::from_secs(5), accept(&listener))
            .await
            .expect("a connection is accepted");
        assert!(stream.peer_addr().is_ok());
        let _ = client.await;
    }
}

#[cfg(test)]
mod switch_tests {
    use super::is_off;

    /// The knobs whose default is OFF (ANTARES_REQUIRE_RLS, ANTARES_TELEMETRY)
    /// must fail SAFE on a spelling they do not know: recognizing only
    /// `1|true` turned the RLS gate off on `TRUE`/`yes`/`on` while the
    /// operator believed it was enforced.
    #[test]
    fn only_an_explicit_off_value_reads_as_off() {
        for off in [
            "0", "false", "FALSE", "False", "off", "OFF", "no", "", " ", " 0\t",
        ] {
            assert!(is_off(off), "{off:?} must read as off");
        }
        for on in [
            "1", "true", "TRUE", "True", "on", "On", "yes", "YES", " 1 ", "enabled",
        ] {
            assert!(!is_off(on), "{on:?} must NOT read as off");
        }
    }
}

#[cfg(test)]
mod sweep_secs_tests {
    use super::parse_sweep_secs;

    /// ANTARES_SWEEP_SECS paces the 4.22 GC in every store mode: absent is
    /// the 15 min default, and a value that is not a positive integer is
    /// fatal — a garbage cadence must never silently become the default one.
    #[test]
    fn sweep_secs_defaults_and_rejects() {
        assert_eq!(parse_sweep_secs(None).expect("default"), 900);
        assert_eq!(parse_sweep_secs(Some("2")).expect("explicit"), 2);
        for bad in ["0", "-1", "", "2s", "abc", "1.5", "99999999999999999999999"] {
            let err =
                parse_sweep_secs(Some(bad)).expect_err(&format!("SWEEP_SECS={bad:?} is fatal"));
            assert!(err.contains("ANTARES_SWEEP_SECS"), "{err}");
        }
    }
}

#[cfg(test)]
mod driver_registry_tests {
    use super::{temporal_choice, TemporalChoice, BUILT_WITH};
    use antares_sql::StoreMode;

    /// Absent, or the store's own name, means one instance serves both
    /// seams — no second store is ever built for the default.
    #[test]
    fn absent_or_same_name_shares_the_store() {
        assert_eq!(
            temporal_choice(StoreMode::Postgres, None).expect("ok"),
            TemporalChoice::SameAsStore
        );
        assert_eq!(
            temporal_choice(StoreMode::Postgres, Some("postgres")).expect("ok"),
            TemporalChoice::SameAsStore
        );
    }

    #[test]
    fn none_turns_history_off_and_other_names_build_a_second_store() {
        assert_eq!(
            temporal_choice(StoreMode::Memory, Some("none")).expect("ok"),
            TemporalChoice::None
        );
        assert_eq!(
            temporal_choice(StoreMode::Memory, Some("timescale")).expect("ok"),
            TemporalChoice::Second(StoreMode::Timescale)
        );
    }

    /// An unknown backend is fatal at startup and the message names the
    /// shelf this binary was built with — never a silent default.
    #[test]
    fn unknown_backend_is_fatal_and_lists_the_shelf() {
        let err = temporal_choice(StoreMode::Memory, Some("mongo")).expect_err("must fail");
        assert!(err.contains("mongo"), "{err}");
        assert!(err.contains(BUILT_WITH), "{err}");
        assert!(
            temporal_choice(StoreMode::Memory, Some("")).is_err(),
            "an empty name is not a default"
        );
    }
}

#[cfg(test)]
mod pg_pool_tests {
    use super::{parse_pg_pool, parse_pg_statement_timeout};

    /// ANTARES_PG_POOL: absent defaults to 20; a value that is not a
    /// positive integer is fatal — misconfiguration must never silently
    /// run with a default.
    #[test]
    fn pg_pool_parse_defaults_and_rejects() {
        assert_eq!(
            parse_pg_statement_timeout(None).expect("default"),
            std::time::Duration::from_secs(30)
        );
        assert_eq!(
            parse_pg_statement_timeout(Some("1500")).expect("explicit"),
            std::time::Duration::from_millis(1500)
        );
        assert!(parse_pg_statement_timeout(Some("0")).is_err());
        assert!(parse_pg_statement_timeout(Some("30s")).is_err());
        assert_eq!(parse_pg_pool(None).expect("default"), 20);
        assert_eq!(parse_pg_pool(Some("7")).expect("explicit"), 7);
        assert!(
            parse_pg_pool(Some("abc")).is_err(),
            "non-numeric ANTARES_PG_POOL must be fatal"
        );
        assert!(
            parse_pg_pool(Some("0")).is_err(),
            "a zero-sized pool must be fatal"
        );
        assert!(parse_pg_pool(Some("-3")).is_err());
    }
}
