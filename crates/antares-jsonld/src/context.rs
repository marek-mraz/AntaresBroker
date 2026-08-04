//! JSON-LD context processing — the NGSI-LD-subset processor (§6.3, risk #1
//! escape hatch taken: hand-rolled, no `json-ld` crate dependency).
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
                    let id = match o.get("@id").and_then(Value::as_str) {
                        Some(id) => self.expand_iri_for_def(id),
                        // No @id: term maps into the vocab (or is a keyword alias we skip).
                        None => {
                            if o.contains_key("@container") || o.contains_key("@type") {
                                self.expand_iri_for_def(term)
                            } else {
                                continue;
                            }
                        }
                    };
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
        // prefix compaction: longest matching prefix-capable term
        let mut best: Option<(usize, String)> = None;
        for (term, def) in &self.terms {
            if def.prefix_ok && !def.iri.is_empty() {
                if let Some(rest) = iri.strip_prefix(&def.iri) {
                    if !rest.is_empty() && best.as_ref().is_none_or(|(l, _)| def.iri.len() > *l) {
                        best = Some((def.iri.len(), format!("{term}:{rest}")));
                    }
                }
            }
        }
        best.map(|(_, s)| s).unwrap_or_else(|| iri.to_owned())
    }
}

fn vocab_or_default(vocab: &str) -> &str {
    if vocab.is_empty() {
        DEFAULT_VOCAB
    } else {
        vocab
    }
}

pub fn is_absolute_iri(s: &str) -> bool {
    match s.split_once(':') {
        Some((scheme, rest)) => {
            !scheme.is_empty()
                && !rest.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
                && !scheme.chars().next().is_some_and(|c| c.is_ascii_digit())
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
}
