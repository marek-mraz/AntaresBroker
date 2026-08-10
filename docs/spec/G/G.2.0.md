---
clause: G.2.0
title: Foreword
pages: 420-421
status: informative
evidence: ''
notes: ''
robot: []
---

G.2.0
Foreword
All strings within an NGSI-LD system are defined and sorted as a sequence of Unicode characters. As such there is no
simple collation mechanism to query entities ignoring case, diacritic marks or matching diphthong single letters such as
the German "ö" to also match with "oe".
Many databases support a degree of natural language support, in general collation support will always depend upon the
underlying database and as such will vary from implementation to implementation. This therefore and cannot be
standardized and exposed as part of the context information management API. Furthermore, collation is slow and
processor intensive, and for massive systems is better achieved using a separate index.


For systems that require it, this clause proposes a mechanism as an extension to a NGSI-LD Context Broker which
can be modified and used to offer collation support to the natural language attributes found within context entities
where necessary through creating, querying and maintaining an additional property of a property for collated attributes.
