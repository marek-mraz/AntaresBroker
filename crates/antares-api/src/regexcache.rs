// SPDX-License-Identifier: EUPL-1.2
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
//! One compile has to be bounded and a pattern has to be compiled at most
//! once, because both numbers are multiplied by the candidate count. A
//! pattern whose compiled automaton is above `MAX_REGEX_PROGRAM_BYTES` is
//! refused with the builder's own error, which every call site already maps
//! — 400 BadRequestData for an `idPattern` (Table 6.3.2-1), no L(R) and so
//! no match for a `~=` operand (4.9). Every outcome is retained, refusals
//! included, bounded in entries and in bytes, because the key is client
//! input and an unbounded map of it is a memory attack, not a cache.

pub use antares_ql::regex::{compile, geo_query, q_node};
