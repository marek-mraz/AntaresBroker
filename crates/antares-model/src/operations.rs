// SPDX-License-Identifier: EUPL-1.2
//! Distributed operation names (CIM 009 clause 4.20).
//!
//! Table 4.20-1 names every API operation a Context Source may declare in the
//! 5.2.9 `operations` member; Table 4.20-2 names five groups that each stand
//! for a set of them. The vocabulary lives here because two independent
//! places read it — the registration validator that decides which names are
//! accepted, and the match index that decides which registrations a
//! distributed operation reaches. A name present in one and absent from the
//! other is a registration the API takes and the broker never forwards to.

/// Table 4.20-1: every named API operation, in the table's own order.
///
/// The order is APPEND-ONLY. A backend may store a set of these as a bitmask
/// whose bit position is the index (the Postgres `csource_index.ops` column
/// does), so reordering or removing a name rewrites the meaning of rows
/// already written: it is a migration, never an edit.
pub const OPERATION_NAMES: &[&str] = &[
    "createEntity",
    "updateEntity",
    "appendAttrs",
    "updateAttrs",
    "deleteAttrs",
    "deleteEntity",
    "createBatch",
    "upsertBatch",
    "updateBatch",
    "deleteBatch",
    "upsertTemporal",
    "appendAttrsTemporal",
    "deleteAttrsTemporal",
    "updateAttrInstanceTemporal",
    "deleteAttrInstanceTemporal",
    "deleteTemporal",
    "mergeEntity",
    "replaceEntity",
    "replaceAttrs",
    "mergeBatch",
    "purgeEntity",
    "retrieveEntity",
    "queryEntity",
    "queryBatch",
    "retrieveTemporal",
    "queryTemporal",
    "retrieveEntityTypes",
    "retrieveEntityTypeDetails",
    "retrieveEntityTypeInfo",
    "retrieveAttrTypes",
    "retrieveAttrTypeDetails",
    "retrieveAttrTypeInfo",
    "createSubscription",
    "updateSubscription",
    "retrieveSubscription",
    "querySubscription",
    "deleteSubscription",
    "retrieveEntityMap",
    "updateEntityMap",
    "deleteEntityMap",
    "createEntityMapQueryEntity",
    "createEntityMapQueryTemporal",
    "retrieveContextSourceIdentity",
];

/// Table 4.20-2: the five group names. They are legal `operations` values
/// alongside the individual names of Table 4.20-1 and are expanded to their
/// members before an operation is matched against a registration.
pub const OPERATION_GROUPS: &[&str] = &[
    "federationOps",
    "associationOps",
    "updateOps",
    "retrieveOps",
    "redirectionOps",
];

/// 4.20: "If no specific subset of operations is defined for a Context Source
/// Registration, the default set of operations matches the group defined as
/// federationOps."
pub const DEFAULT_OPERATION_GROUP: &str = "federationOps";

/// The Table 4.20-2 members of one group name, or `None` when the name is not
/// a group (an individual operation name, or a name outside the vocabulary).
pub fn group_members(name: &str) -> Option<&'static [&'static str]> {
    /// federationOps: the consumption and subscription operations plus the
    /// EntityMap support operations, minus createEntityMapQueryTemporal.
    const FEDERATION: &[&str] = &[
        "retrieveEntity",
        "queryEntity",
        "queryBatch",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "createSubscription",
        "updateSubscription",
        "retrieveSubscription",
        "querySubscription",
        "deleteSubscription",
        "retrieveEntityMap",
        "updateEntityMap",
        "deleteEntityMap",
        "createEntityMapQueryEntity",
        "retrieveContextSourceIdentity",
    ];
    /// associationOps: federationOps WITHOUT the EntityMap support operations.
    const ASSOCIATION: &[&str] = &[
        "retrieveEntity",
        "queryEntity",
        "queryBatch",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "createSubscription",
        "updateSubscription",
        "retrieveSubscription",
        "querySubscription",
        "deleteSubscription",
        "retrieveContextSourceIdentity",
    ];
    const UPDATE: &[&str] = &[
        "updateEntity",
        "updateAttrs",
        "replaceEntity",
        "replaceAttrs",
    ];
    const RETRIEVE: &[&str] = &["retrieveEntity", "queryEntity"];
    const REDIRECTION: &[&str] = &[
        "createEntity",
        "updateEntity",
        "appendAttrs",
        "updateAttrs",
        "deleteAttrs",
        "deleteEntity",
        "mergeEntity",
        "replaceEntity",
        "replaceAttrs",
        "retrieveEntity",
        "queryEntity",
        "purgeEntity",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "retrieveEntityMap",
        "updateEntityMap",
        "deleteEntityMap",
        "createEntityMapQueryEntity",
        "retrieveContextSourceIdentity",
    ];
    match name {
        "federationOps" => Some(FEDERATION),
        "associationOps" => Some(ASSOCIATION),
        "updateOps" => Some(UPDATE),
        "retrieveOps" => Some(RETRIEVE),
        "redirectionOps" => Some(REDIRECTION),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table 4.20-1 as printed: 43 names, no repeats, and the four sections
    /// in the table's order (provision, consumption, subscription, support).
    #[test]
    fn table_4_20_1_is_the_whole_vocabulary() {
        assert_eq!(OPERATION_NAMES.len(), 43);
        let mut sorted = OPERATION_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), OPERATION_NAMES.len(), "duplicate name");
        assert_eq!(OPERATION_NAMES[0], "createEntity");
        assert_eq!(OPERATION_NAMES[20], "purgeEntity");
        assert_eq!(OPERATION_NAMES[21], "retrieveEntity");
        assert_eq!(OPERATION_NAMES[32], "createSubscription");
        assert_eq!(OPERATION_NAMES[37], "retrieveEntityMap");
        assert_eq!(OPERATION_NAMES[42], "retrieveContextSourceIdentity");
    }

    /// A group expands only to operations of Table 4.20-1, and a group name
    /// is never itself an operation name — the two value spaces are
    /// disjoint, which is what lets a validator accept both lists.
    #[test]
    fn table_4_20_2_groups_expand_within_the_vocabulary() {
        for g in OPERATION_GROUPS {
            assert!(
                !OPERATION_NAMES.contains(g),
                "{g} is both a group and an operation"
            );
            let members = group_members(g).unwrap_or_else(|| panic!("{g} has no members"));
            assert!(!members.is_empty());
            for m in members {
                assert!(OPERATION_NAMES.contains(m), "{g} expands to unknown {m}");
            }
        }
        assert_eq!(group_members("federationOps").map(<[&str]>::len), Some(19));
        assert_eq!(group_members("associationOps").map(<[&str]>::len), Some(15));
        assert_eq!(group_members("updateOps").map(<[&str]>::len), Some(4));
        assert_eq!(group_members("retrieveOps").map(<[&str]>::len), Some(2));
        assert_eq!(group_members("redirectionOps").map(<[&str]>::len), Some(23));
        assert_eq!(group_members("retrieveEntity"), None);
        assert_eq!(group_members("notAGroup"), None);
    }

    /// Table 4.20-2: associationOps is federationOps without the EntityMap
    /// support operations, and createEntityMapQueryTemporal is in no group.
    #[test]
    fn association_is_federation_without_the_entity_maps() {
        let fed = group_members("federationOps").expect("federationOps");
        let assoc = group_members("associationOps").expect("associationOps");
        let only_fed: Vec<_> = fed.iter().filter(|m| !assoc.contains(m)).copied().collect();
        assert_eq!(
            only_fed,
            [
                "retrieveEntityMap",
                "updateEntityMap",
                "deleteEntityMap",
                "createEntityMapQueryEntity"
            ]
        );
        for g in OPERATION_GROUPS {
            let members = group_members(g).expect("group");
            assert!(!members.contains(&"createEntityMapQueryTemporal"), "{g}");
        }
    }

    /// The default an absent `operations` member stands for is a group name,
    /// so both readers expand it the same way.
    #[test]
    fn the_default_is_a_group() {
        assert!(OPERATION_GROUPS.contains(&DEFAULT_OPERATION_GROUP));
        assert!(group_members(DEFAULT_OPERATION_GROUP).is_some());
    }
}
