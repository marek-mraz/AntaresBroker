//! JSON-LD context processing — the NGSI-LD-subset processor (hand-rolled,
//! no `json-ld` crate dependency).
//!
//! A [`Context`] is the merged, resolved term map for one request: user
//! contexts (in order, later wins) with the core context merged last (core
//! terms take precedence, CIM 009 4.4: core terms are protected).

use antares_model::NgsiError;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// The default vocabulary IRI the core context sets: unknown terms expand here.
pub const DEFAULT_VOCAB: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// NGSI-LD core vocabulary base (terms like location, observedAt, …).
pub const NGSI_LD_BASE: &str = "https://uri.etsi.org/ngsi-ld/";

#[derive(Debug, Clone, Default)]
pub struct TermDef {
    pub iri: String,
    /// `@type: @id` — values are IRIs (compact them on output).
    pub type_is_id: bool,
    /// `@type: @vocab` — values are vocab terms.
    pub type_is_vocab: bool,
    /// `@container: @list`
    pub container_list: bool,
    /// term may be used as a prefix (its IRI ends with a gen-delim, or it was
    /// defined as a plain string mapping — JSON-LD 1.1 simple-term rule).
    pub prefix_ok: bool,
}

#[derive(Debug, Default)]
pub struct Context {
    terms: HashMap<String, TermDef>,
    /// IRI → term for compaction (built after merge; shortest term wins).
    inverse: HashMap<String, String>,
    pub vocab: String,
    /// The @context value to hand back in responses (Link header / body):
    /// what the client sent, before the implicit core merge.
    pub source: Value,
    /// Total bytes of expanded term IRIs merged so far — see [`Context::charge`].
    bytes: usize,
}

impl Context {
    /// Merge one raw `@context` object (its term definitions) into this
    /// context. Later calls override earlier definitions.
    pub fn merge_object(&mut self, obj: &Map<String, Value>) -> Result<(), NgsiError> {
        if let Some(v) = obj.get("@vocab").and_then(Value::as_str) {
            self.vocab = v.to_owned();
        }
        // Two passes so terms can reference prefixes defined in the same object.
        // Pass 1: raw string mappings that look like absolute IRIs (prefix seeds).
        for (term, def) in obj {
            if term.starts_with('@') {
                continue;
            }
            if let Some(s) = def.as_str() {
                if is_absolute_iri(s) {
                    self.terms.insert(
                        term.clone(),
                        TermDef {
                            iri: s.to_owned(),
                            prefix_ok: true,
                            ..Default::default()
                        },
                    );
                }
            }
        }
        // Pass 2: everything, resolving compact IRIs against known terms.
        for (term, def) in obj {
            if term.starts_with('@') {
                continue;
            }
            match def {
                Value::String(s) => {
                    let iri = self.expand_iri_for_def(s);
                    self.charge(&iri)?;
                    self.terms.insert(
                        term.clone(),
                        TermDef {
                            iri,
                            prefix_ok: true,
                            ..Default::default()
                        },
                    );
                }
                Value::Object(o) => {
                    // 5.5.7: user @contexts shall not contain JSON-LD Scoped
                    // Contexts — a per-term @context could override core
                    // terms or reshape elements during expansion →
                    // BadRequestData.
                    if o.contains_key("@context") {
                        return Err(NgsiError::BadRequestData(format!(
                            "term {term:?}: JSON-LD Scoped Contexts are not \
                             allowed in a user @context (5.5.7)"
                        )));
                    }
                    let id = match o.get("@id").and_then(Value::as_str) {
                        Some(id) => self.expand_iri_for_def(id),
                        // No @id: the term maps into the active vocabulary
                        // (or is a keyword alias we skip). The core @context
                        // is merged last (4.4), so its @vocab is not set yet
                        // — fall back to it rather than leave a RELATIVE IRI
                        // that 4.5.1 then has to reject.
                        None => {
                            if o.contains_key("@container") || o.contains_key("@type") {
                                match self.expand_iri_for_def(term) {
                                    iri if is_absolute_iri(&iri) => iri,
                                    _ => format!("{}{term}", vocab_or_default(&self.vocab)),
                                }
                            } else {
                                continue;
                            }
                        }
                    };
                    self.charge(&id)?;
                    let t = o.get("@type").and_then(Value::as_str).unwrap_or("");
                    let c = o.get("@container").and_then(Value::as_str).unwrap_or("");
                    self.terms.insert(
                        term.clone(),
                        TermDef {
                            iri: id,
                            type_is_id: t == "@id",
                            type_is_vocab: t == "@vocab",
                            container_list: c == "@list",
                            prefix_ok: o.get("@prefix").and_then(Value::as_bool).unwrap_or(false),
                        },
                    );
                }
                Value::Null => {
                    self.terms.remove(term);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// 5.5.6: an `@context` that "is invalid" is BadRequestData. The term map
    /// is built from client-supplied documents, and a chain of prefix
    /// definitions (`"t1": "t0:…"`, `"t2": "t1:…"`, …) makes every definition
    /// carry the whole chain, so N terms expand to O(N²) bytes. The merged
    /// IRIs are budgeted against the ceiling one @context document may
    /// occupy, on the Context rather than per call so a chain split across
    /// several documents cannot walk past it.
    fn charge(&mut self, iri: &str) -> Result<(), NgsiError> {
        self.bytes += iri.len();
        if self.bytes > crate::loader::MAX_CONTEXT_BYTES {
            return Err(NgsiError::BadRequestData(
                "@context term definitions exceed the maximum size".into(),
            ));
        }
        Ok(())
    }

    /// Expand an IRI-position string inside a term definition (@id values).
    fn expand_iri_for_def(&self, s: &str) -> String {
        // JSON-LD keywords ("@type", "@id", …) stay as-is: a term aliased to a
        // keyword must expand to the keyword, never into the vocab.
        if s.starts_with('@') {
            return s.to_owned();
        }
        if let Some((prefix, suffix)) = s.split_once(':') {
            if !suffix.starts_with("//") {
                if let Some(def) = self.terms.get(prefix) {
                    if def.prefix_ok {
                        return format!("{}{}", def.iri, suffix);
                    }
                }
            }
            if is_absolute_iri(s) {
                return s.to_owned();
            }
        }
        if let Some(def) = self.terms.get(s) {
            return def.iri.clone();
        }
        if !self.vocab.is_empty() {
            return format!("{}{}", self.vocab, s);
        }
        s.to_owned()
    }

    /// Build the compaction inverse map. Call once after all merges.
    pub fn freeze(&mut self) {
        let mut inv: HashMap<String, String> = HashMap::new();
        for (term, def) in &self.terms {
            match inv.get(&def.iri) {
                Some(existing)
                    if (existing.len(), existing.as_str()) <= (term.len(), term.as_str()) => {}
                _ => {
                    inv.insert(def.iri.clone(), term.clone());
                }
            }
        }
        self.inverse = inv;
    }

    pub fn term(&self, term: &str) -> Option<&TermDef> {
        self.terms.get(term)
    }

    /// Expand a key/term in vocab position (attribute names, type values).
    pub fn expand_key(&self, key: &str) -> String {
        if let Some(def) = self.terms.get(key) {
            return def.iri.clone();
        }
        if let Some((prefix, suffix)) = key.split_once(':') {
            if !suffix.starts_with("//") {
                if let Some(def) = self.terms.get(prefix) {
                    if def.prefix_ok {
                        return format!("{}{}", def.iri, suffix);
                    }
                }
            }
            if is_absolute_iri(key) {
                return key.to_owned();
            }
        }
        format!("{}{}", vocab_or_default(&self.vocab), key)
    }

    /// Compact an IRI back to a term (attribute names, type values).
    ///
    /// Vocab-relative shortening is only valid when the resulting bare term
    /// would round-trip: if the term is already bound to a DIFFERENT IRI in
    /// this context, fall back to prefix compaction
    /// (`ngsi-ld:default-context/x`) — JSON-LD compaction semantics the
    /// conformance suite depends on.
    pub fn compact_iri(&self, iri: &str) -> String {
        if let Some(term) = self.inverse.get(iri) {
            return term.clone();
        }
        let vocab = vocab_or_default(&self.vocab);
        for v in [vocab, DEFAULT_VOCAB] {
            if let Some(rest) = iri.strip_prefix(v) {
                let round_trips = !rest.is_empty()
                    && !rest.contains(':')
                    && self.terms.get(rest).is_none_or(|d| d.iri == iri);
                if round_trips {
                    return rest.to_owned();
                }
                break;
            }
        }
        // Prefix compaction: longest matching prefix-capable term. `terms` is
        // a randomly-seeded HashMap, so ties are broken on the term itself
        // (shortest, then lexicographic — the same rule as `freeze`) or the
        // chosen prefix would differ between processes.
        let mut best: Option<(usize, &str)> = None;
        for (term, def) in &self.terms {
            if def.prefix_ok
                && !def.iri.is_empty()
                && iri.strip_prefix(&def.iri).is_some_and(|r| !r.is_empty())
            {
                let better = match best {
                    None => true,
                    Some((l, t)) => {
                        (
                            def.iri.len(),
                            std::cmp::Reverse((term.len(), term.as_str())),
                        ) > (l, std::cmp::Reverse((t.len(), t)))
                    }
                };
                if better {
                    best = Some((def.iri.len(), term));
                }
            }
        }
        match best {
            Some((l, term)) => format!("{term}:{}", &iri[l..]),
            None => iri.to_owned(),
        }
    }
}

fn vocab_or_default(vocab: &str) -> &str {
    if vocab.is_empty() {
        DEFAULT_VOCAB
    } else {
        vocab
    }
}

/// RFC 3986 3.1: `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` — the
/// scheme starts with a letter, and the part after the colon must not be
/// empty. 4.5.1/5.5.4 lean on this to keep every expanded Attribute name
/// absolute.
pub fn is_absolute_iri(s: &str) -> bool {
    match s.split_once(':') {
        Some((scheme, rest)) => {
            !rest.is_empty()
                && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(v: Value) -> Context {
        let mut c = Context::default();
        c.merge_object(v.as_object().unwrap()).unwrap();
        c.freeze();
        c
    }

    /// 5.5.7: the user @context "shall not contain JSON-LD Scoped Contexts"
    /// — a term definition carrying its own @context "should result in an
    /// error of type BadRequestData" (it could reshape core terms during
    /// expansion). Compaction with no matching term renders the FQN.
    #[test]
    fn clause_5_5_7_scoped_contexts_rejected_and_fqn_fallback() {
        let mut c = Context::default();
        let err = c
            .merge_object(
                json!({"Vehicle": {"@id": "https://example.org/Vehicle",
                    "@context": {"speed": "https://example.org/hidden-speed"}}})
                .as_object()
                .unwrap(),
            )
            .expect_err("scoped context must be rejected");
        assert!(
            matches!(err, NgsiError::BadRequestData(_)),
            "BadRequestData, got {err:?}"
        );
        // and the smuggled scoped term must NOT have landed in the context
        assert_ne!(
            ctx(json!({"x": "https://example.org/x"})).expand_key("speed"),
            "https://example.org/hidden-speed"
        );
        // compaction without a matching term renders the FQN verbatim
        let c = ctx(json!({"name": "https://example.org/name"}));
        assert_eq!(
            c.compact_iri("https://elsewhere.org/unmapped"),
            "https://elsewhere.org/unmapped"
        );
    }

    #[test]
    fn plain_term_mapping() {
        let c = ctx(json!({"name": "https://example.org/name"}));
        assert_eq!(c.expand_key("name"), "https://example.org/name");
        assert_eq!(c.compact_iri("https://example.org/name"), "name");
    }

    #[test]
    fn prefix_expansion() {
        let c = ctx(json!({"ex": "https://example.org/", "a": {"@id": "ex:a"}}));
        assert_eq!(c.expand_key("a"), "https://example.org/a");
        assert_eq!(c.expand_key("ex:b"), "https://example.org/b");
        assert_eq!(c.compact_iri("https://example.org/b"), "ex:b");
    }

    #[test]
    fn vocab_fallback() {
        let c = ctx(json!({"@vocab": "https://voc.example/"}));
        assert_eq!(c.expand_key("speed"), "https://voc.example/speed");
        assert_eq!(c.compact_iri("https://voc.example/speed"), "speed");
    }

    #[test]
    fn default_vocab_when_absent() {
        let c = ctx(json!({}));
        assert_eq!(
            c.expand_key("speed"),
            "https://uri.etsi.org/ngsi-ld/default-context/speed"
        );
    }

    // ---- merge_object -------------------------------------------------

    /// 4.4: the Core @context is merged last, so its definitions win over a
    /// user redefinition of the same term. The user's own new terms survive.
    #[test]
    fn later_merge_wins_so_core_terms_cannot_be_redefined() {
        let mut c = Context::default();
        c.merge_object(
            json!({"observedAt": "https://evil.example/observedAt",
                   "speed": "https://example.org/speed"})
            .as_object()
            .unwrap(),
        )
        .unwrap();
        c.merge_object(
            json!({"observedAt": "https://uri.etsi.org/ngsi-ld/observedAt"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        c.freeze();
        assert_eq!(
            c.expand_key("observedAt"),
            "https://uri.etsi.org/ngsi-ld/observedAt"
        );
        assert_ne!(
            c.expand_key("observedAt"),
            "https://evil.example/observedAt"
        );
        // the user's unrelated term is untouched by the core merge
        assert_eq!(c.expand_key("speed"), "https://example.org/speed");
    }

    /// A null term definition removes the term; the removed term must fall
    /// back to the vocabulary rather than keep its old IRI.
    #[test]
    fn null_definition_removes_term() {
        let mut c = Context::default();
        c.merge_object(
            json!({"speed": "https://example.org/speed"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        c.merge_object(json!({"speed": null}).as_object().unwrap())
            .unwrap();
        c.freeze();
        assert_ne!(c.expand_key("speed"), "https://example.org/speed");
        assert_eq!(
            c.expand_key("speed"),
            "https://uri.etsi.org/ngsi-ld/default-context/speed"
        );
        // freeze() rebuilds the inverse map: the dropped IRI no longer compacts
        assert_eq!(
            c.compact_iri("https://example.org/speed"),
            "https://example.org/speed"
        );
    }

    /// Keywords and definition values that are neither string, object nor null
    /// are ignored — a hostile @context of numbers/booleans/arrays must not
    /// panic and must not create terms.
    #[test]
    fn keyword_and_non_string_definitions_ignored() {
        let c = ctx(json!({
            "@protected": true,
            "@base": "https://base.example/",
            "n": 1,
            "b": false,
            "a": [1, 2, 3],
            "empty": {},
            "keep": "https://example.org/keep"
        }));
        assert!(c.term("n").is_none());
        assert!(c.term("b").is_none());
        assert!(c.term("a").is_none());
        // an object definition with neither @id, @type nor @container is skipped
        assert!(c.term("empty").is_none());
        assert_eq!(c.expand_key("keep"), "https://example.org/keep");
        // unknown terms must not vanish — they expand into the vocabulary
        assert_eq!(
            c.expand_key("n"),
            "https://uri.etsi.org/ngsi-ld/default-context/n"
        );
    }

    /// Expanded term-definition forms: @type/@container flags and a term
    /// aliased to a JSON-LD keyword (which must stay the keyword, not expand
    /// into the vocabulary).
    #[test]
    fn expanded_definition_forms() {
        let c = ctx(json!({
            "ex": "https://example.org/",
            "rel": {"@id": "ex:rel", "@type": "@id"},
            "kind": {"@id": "ex:kind", "@type": "@vocab"},
            "items": {"@id": "ex:items", "@container": "@list"},
            "alias": {"@id": "@type"},
            "implicit": {"@type": "@id"}
        }));
        let rel = c.term("rel").unwrap();
        assert!(rel.type_is_id && !rel.type_is_vocab && !rel.container_list);
        assert_eq!(rel.iri, "https://example.org/rel");
        assert!(c.term("kind").unwrap().type_is_vocab);
        assert!(c.term("items").unwrap().container_list);
        assert_eq!(c.term("alias").unwrap().iri, "@type");
        // no @id but @type present: the term maps into the vocabulary
        assert_eq!(
            c.term("implicit").unwrap().iri,
            "https://uri.etsi.org/ngsi-ld/default-context/implicit"
        );
        // an expanded definition is not prefix-capable unless @prefix says
        // so: "rel:x" stays the IRI it already is and is NOT rewritten
        // through the term.
        assert!(!rel.prefix_ok);
        assert_eq!(c.expand_key("rel:x"), "rel:x");
        assert_ne!(c.expand_key("rel:x"), "https://example.org/relx");
    }

    /// The @context is attacker-supplied. Prefix chaining ("t1" defined
    /// through "t0", "t2" through "t1", …) makes each definition carry the
    /// whole chain, so N such terms expand to O(N²) bytes — a few megabytes of
    /// request body would otherwise become gigabytes of term map. The merge
    /// must stop with BadRequestData instead.
    #[test]
    fn prefix_chain_cannot_amplify_unbounded() {
        let suffix = "a".repeat(32);
        let mut obj = Map::new();
        obj.insert(
            "t000000".into(),
            Value::String(format!("https://ex.example/{suffix}")),
        );
        for i in 1..2000u32 {
            obj.insert(
                format!("t{i:06}"),
                Value::String(format!("t{:06}:{suffix}", i - 1)),
            );
        }
        let mut c = Context::default();
        let err = c
            .merge_object(&obj)
            .expect_err("an amplifying @context must be rejected");
        assert!(
            matches!(err, NgsiError::BadRequestData(_)),
            "BadRequestData, got {err:?}"
        );
    }

    /// The bound must not reject an ordinary large vocabulary: 20 000 plain
    /// term mappings expand to roughly their own size and are accepted.
    #[test]
    fn large_plain_context_still_accepted() {
        let mut obj = Map::new();
        for i in 0..20_000u32 {
            obj.insert(
                format!("term{i}"),
                Value::String(format!("https://ex.example/vocab#term{i}")),
            );
        }
        let mut c = Context::default();
        c.merge_object(&obj).expect("plain context accepted");
        c.freeze();
        assert_eq!(
            c.expand_key("term19999"),
            "https://ex.example/vocab#term19999"
        );
    }

    /// Self-referential and mutually-referential prefixes must terminate:
    /// term-definition IRI expansion resolves one level against the terms
    /// already merged, it never follows a chain recursively.
    #[test]
    fn self_referential_prefixes_terminate() {
        let c = ctx(json!({"a": "a:x"}));
        assert!(!c.term("a").unwrap().iri.is_empty());
        let c = ctx(json!({"a": "b:x", "b": "a:y"}));
        // both resolved to something finite; neither hung nor recursed
        assert!(c.term("a").unwrap().iri.len() < 64);
        assert!(c.term("b").unwrap().iri.len() < 64);
    }

    // ---- freeze -------------------------------------------------------

    /// Several terms bound to one IRI: compaction picks the shortest, ties
    /// broken lexicographically, and the choice is stable across freezes.
    #[test]
    fn freeze_inverse_is_deterministic() {
        let c = ctx(json!({
            "aaa": "https://example.org/x",
            "bb": "https://example.org/x",
            "cc": "https://example.org/x"
        }));
        assert_eq!(c.compact_iri("https://example.org/x"), "bb");
        for _ in 0..5 {
            let c2 = ctx(json!({
                "cc": "https://example.org/x",
                "bb": "https://example.org/x",
                "aaa": "https://example.org/x"
            }));
            assert_eq!(c2.compact_iri("https://example.org/x"), "bb");
        }
    }

    // ---- expand_key ---------------------------------------------------

    /// Adversarial keys: empty, lone/leading/trailing colons, "://", huge
    /// prefixes and multi-byte UTF-8 — expansion slices on ':' and must never
    /// panic on a character boundary.
    #[test]
    fn expand_key_adversarial_strings_do_not_panic() {
        let c = ctx(json!({"ex": "https://example.org/", "": "https://empty.example/"}));
        let vocab = "https://uri.etsi.org/ngsi-ld/default-context/";
        assert_eq!(c.expand_key(""), "https://empty.example/");
        assert_eq!(c.expand_key(":"), "https://empty.example/");
        assert_eq!(c.expand_key("://"), format!("{vocab}://"));
        assert_eq!(c.expand_key(":x"), "https://empty.example/x");
        assert_eq!(c.expand_key("ex:"), "https://example.org/");
        assert_eq!(c.expand_key("é"), format!("{vocab}é"));
        assert_eq!(c.expand_key("ex:°C"), "https://example.org/°C");
        assert_eq!(c.expand_key("日本:語"), format!("{vocab}日本:語"));
        assert_eq!(c.expand_key("\u{feff}:x"), format!("{vocab}\u{feff}:x"));
        let huge = "x".repeat(100_000);
        assert_eq!(c.expand_key(&huge), format!("{vocab}{huge}"));
        assert_eq!(
            c.expand_key(&format!("ex:{huge}")),
            format!("https://example.org/{huge}")
        );
    }

    /// A term definition wins over reading the same key as prefix:suffix.
    #[test]
    fn term_lookup_precedes_prefix_split() {
        let c = ctx(json!({"ex": "https://example.org/", "ex:a": "https://direct.example/a"}));
        assert_eq!(c.expand_key("ex:a"), "https://direct.example/a");
        assert_ne!(c.expand_key("ex:a"), "https://example.org/a");
    }

    /// An absolute IRI used as a key stays itself; a non-prefix-capable term
    /// before the colon must not be applied.
    #[test]
    fn absolute_iri_keys_pass_through() {
        let c = ctx(json!({"ex": {"@id": "https://example.org/"}}));
        assert_eq!(
            c.expand_key("https://other.example/a"),
            "https://other.example/a"
        );
        assert_eq!(c.expand_key("urn:ngsi-ld:X"), "urn:ngsi-ld:X");
        // @id-form definitions are not prefixes (JSON-LD 1.1 simple-term rule)
        assert_eq!(c.expand_key("ex:a"), "ex:a");
    }

    // ---- compact_iri --------------------------------------------------

    /// Compaction must not leave an expanded IRI in the document when the
    /// context defines a term for it.
    #[test]
    fn defined_terms_never_stay_expanded() {
        let c = ctx(json!({"ex": "https://example.org/", "name": "https://example.org/name"}));
        assert_eq!(c.compact_iri("https://example.org/name"), "name");
        assert_ne!(
            c.compact_iri("https://example.org/name"),
            "https://example.org/name"
        );
        // no exact term: longest prefix-capable term wins
        assert_eq!(c.compact_iri("https://example.org/other"), "ex:other");
    }

    /// Vocab-relative shortening only when the bare term round-trips: if the
    /// context binds that term to a DIFFERENT IRI the full IRI is kept.
    #[test]
    fn vocab_shortening_requires_round_trip() {
        let c = ctx(json!({"@vocab": "https://voc.example/",
                           "speed": "https://other.example/speed"}));
        assert_eq!(
            c.compact_iri("https://voc.example/speed"),
            "https://voc.example/speed"
        );
        assert_ne!(c.compact_iri("https://voc.example/speed"), "speed");
        // a vocab-relative remainder containing ':' is not a usable term
        assert_eq!(
            c.compact_iri("https://voc.example/a:b"),
            "https://voc.example/a:b"
        );
        // the vocab IRI itself has an empty remainder
        assert_eq!(
            c.compact_iri("https://voc.example/"),
            "https://voc.example/"
        );
    }

    /// Longest matching prefix wins, and a term whose IRI is empty is never
    /// used as a prefix (it would match everything).
    #[test]
    fn prefix_compaction_picks_longest_and_skips_empty() {
        let c = ctx(json!({"ex": "https://example.org/",
                           "sub": "https://example.org/sub/",
                           "nil": {"@id": "", "@prefix": true}}));
        assert_eq!(c.compact_iri("https://example.org/sub/x"), "sub:x");
        assert_eq!(c.compact_iri("https://example.org/y"), "ex:y");
        assert!(!c
            .compact_iri("https://elsewhere.example/z")
            .starts_with("nil:"));
    }

    /// Compaction must be reproducible across processes: when several
    /// prefix-capable terms share one IRI the winner may not depend on hash
    /// map iteration order, which is randomly seeded per map.
    #[test]
    fn prefix_compaction_tie_break_is_stable() {
        let defs = json!({
            "h": "https://example.org/", "g": "https://example.org/",
            "f": "https://example.org/", "e": "https://example.org/",
            "d": "https://example.org/", "c": "https://example.org/",
            "b": "https://example.org/", "a": "https://example.org/"
        });
        for _ in 0..50 {
            assert_eq!(
                ctx(defs.clone()).compact_iri("https://example.org/z"),
                "a:z"
            );
        }
    }

    // ---- is_absolute_iri ----------------------------------------------

    /// RFC 3986 scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ), and the
    /// hierarchical part must not be empty. The edge set below is what a
    /// hostile @context or entity key can carry.
    #[test]
    fn is_absolute_iri_edge_set() {
        for s in [
            "https://example.org/x",
            "urn:ngsi-ld:X",
            "a:b",
            "a+b-c.d:x",
            "A:x",
            "a::b",
            "x:日本",
            "http://x",
        ] {
            assert!(is_absolute_iri(s), "expected absolute: {s:?}");
        }
        for s in [
            "",
            ":",
            "://",
            ":x",
            "a:",
            "1a:x",
            "3D:x",
            "a b:x",
            "日本:x",
            "no-colon",
            "+x:y",
            "-x:y",
            ".x:y",
            "\u{feff}:x",
        ] {
            assert!(!is_absolute_iri(s), "expected NOT absolute: {s:?}");
        }
    }
}
