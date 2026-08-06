//! Query-filter shapes shared by every backend (C10/C11 pushdown contract).
//!
//! Pure data — no sqlx, no I/O — split out of `pg_entity`/`pg_temporal` (N2)
//! so the wasm32 build (no `postgres` feature) keeps the same `AnyStore`
//! query surface: the memory arm consumes these filters too, it just never
//! gets a `decided` outcome.

use serde_json::Value;

pub struct EntityFilter<'a> {
    /// exact entity ids (`id=` / the ids of a batch query)
    pub ids: Option<&'a [&'a str]>,
    /// Entity Type Selection (4.17) as OR-of-AND groups, expanded IRIs
    pub types: Option<&'a [Vec<String>]>,
    /// `attrs=`: the entity must carry at least one, expanded IRIs
    pub attrs: Option<&'a [String]>,
    /// `q=` AST; compiled when its shape is exactly reproducible, else skipped
    pub q: Option<&'a antares_ql::QNode>,
    /// `scopeQ=` verbatim (4.19); compiled over the `scopes` column (C11)
    pub scope_q: Option<&'a str>,
    /// `georel`/`geometry`/`coordinates`/`geoproperty` (4.10), compiled over
    /// the extracted `location` column (C11b)
    pub geo: Option<&'a crate::compile::geo::GeoSpec<'a>>,
    /// term → IRI, the request context's expander (the AST holds terms)
    pub expand: &'a dyn Fn(&str) -> String,
    /// C11 pagination pushdown: applied ONLY when every present predicate
    /// compiled exactly (`decided`) — otherwise the caller's evaluator still
    /// has rows to drop and a SQL LIMIT would page over the wrong set. The
    /// caller passes it only when its own store-invisible filters (idPattern,
    /// federation, orderBy) are absent.
    pub page: Option<Page>,
    /// C11 projection pushdown (4.21 `pick`, top-level): keep these expanded
    /// attr IRIs + every non-attribute member. Applied only when `decided` —
    /// a projected doc can no longer answer a q= re-check.
    pub keep_attrs: Option<&'a [String]>,
    /// C11 projection pushdown (`omit`, top-level entries only): drop exactly
    /// these attr IRIs. Same `decided` gate.
    pub drop_attrs: Option<&'a [String]>,
}

impl Default for EntityFilter<'_> {
    fn default() -> Self {
        Self {
            ids: None,
            types: None,
            attrs: None,
            q: None,
            scope_q: None,
            geo: None,
            expand: &|t: &str| t.to_owned(),
            page: None,
            keep_attrs: None,
            drop_attrs: None,
        }
    }
}

/// One page: OFFSET/LIMIT in row units, ORDER BY id (the store's stable
/// default order, same as the memory snapshot).
pub struct Page {
    pub offset: i64,
    pub limit: i64,
}

/// What `query` produced. `decided` = SQL applied every present predicate
/// exactly, so re-evaluation cannot drop a row; `paged` = LIMIT/OFFSET
/// happened in SQL (implies `decided`), `total` = the pre-LIMIT match count.
pub struct QueryOutcome {
    pub rows: Vec<Value>,
    pub decided: bool,
    pub paged: bool,
    pub total: Option<i64>,
}

pub struct TemporalFilter<'a> {
    /// exact entity ids
    pub ids: Option<&'a [&'a str]>,
    /// flat OR list of expanded type IRIs (temporal query has no AND groups)
    pub types: Option<&'a [String]>,
    /// `attrs=`: the entity must carry at least one, expanded IRIs
    pub attrs: Option<&'a [String]>,
    /// the 4.11 window; `None` = no instance pruning
    pub range: Option<crate::compile::temporal::InstanceRange<'a>>,
    /// `lastN`: per-(attr, datasetId) RANK() cap — ties all kept, so the
    /// per-attr lastN the API applies afterwards always finds its instances
    pub last_n: Option<i64>,
    /// ordering key for the lastN cap (the request's timeproperty)
    pub timeproperty: &'a str,
}

impl Default for TemporalFilter<'_> {
    fn default() -> Self {
        Self {
            ids: None,
            types: None,
            attrs: None,
            range: None,
            last_n: None,
            timeproperty: "observedAt",
        }
    }
}
