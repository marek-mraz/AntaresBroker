---
clause: A.2
title: Entity identifiers
pages: '364'
status: not-implemented
evidence: ''
notes: ''
robot: []
---

A.2
Entity identifiers
In order to enable the participation of NGSI-LD in linked data scenarios, all Entities are identified by URIs. If those
URIs are expected to participate in external linked data relationships they should be dereferenceable.
It is noteworthy that the identifier from the point of view of NGSI-LD is different from the inherent identifier that a
specific Entity may have. For instance, an NGSI-LD Entity of Type "Vehicle" may have a Property named
licencePlateNumber, which it is actually a unique identifier from the point of view of the Entity domain, as it uniquely
identifies the specific vehicle instance. However, from the point of view of the NGSI-LD system, it may have another
identifier which might or might not include such licence plate number identifier.
