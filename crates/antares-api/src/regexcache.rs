//! Process-wide caches of compiled client query text — the regex, `q` and
//! `geoQ` caches live in `antares_ql::regex`; this module keeps the
//! broker-side names.
//!
//! Two NGSI-LD surfaces carry a client-supplied regular expression. The query
//! language, 4.9 **Match pattern** (production rule `patternOp`): "A matching
//! entity shall contain the target element and the target value shall be in
//! the L(R) of the regular pattern specified by the Query Term" — and its
//! `notPatternOp` mirror. And `idPattern`, on an EntitySelector (5.2.33), an
//! EntityInfo (5.2.8) and the query parameters of Table 6.4.3.2-1.
//!
//! Both are evaluated per candidate entity and, for a subscription, per event
//! per subscription — while the pattern text belongs to the query or the
//! subscription, not to the candidate. Compiling at the point of use
//! therefore pays `Regex::new` again for every candidate; compiling through
//! here pays it once per distinct pattern and hands out a shared program.
//!
//! The cache changes no outcome. `compile` accepts and rejects exactly what
//! `regex::Regex::new` accepts and rejects, and returns that call's own
//! `regex::Error`, so an invalid `idPattern` keeps the 400 BadRequestData its
//! call site already returns (Table 6.3.2-1) and an invalid `~=` operand
//! keeps having no L(R), i.e. matching nothing (4.9).
//!
//! Retention is bounded in both dimensions — entries and compiled program
//! size, `MAX_REGEX_CACHE` and `MAX_REGEX_PROGRAM_BYTES` —
//! because the key is client input and an unbounded map of it is a memory
//! attack, not a cache.

pub use antares_ql::regex::{cached, compile, compiles, len};
pub use antares_ql::regex::{geo_query, q_node};
