---
clause: D.4
title: Algorithm for transforming an NGSI-LD Relationship into JSON-LD (ALG1.2)
pages: 416-417
status: informative
evidence: ''
notes: ''
robot: []
---

D.4
Algorithm for transforming an NGSI-LD Relationship
into JSON-LD (ALG1.2)
Let Rs be the Relationship that has to be transformed. It is defined by (R, "AliasR", Robj), where R denotes a
Relationship Type Id, "AliasR" is the Relationship's name and Robj is the identifier of the target object of the
Relationship.
Rs might be associated to extra Properties or Relationships.
Let O be the output JSON-LD object and C the current JSON-LD context:
1)
Execute the following statements:
a)
If no member with "AliasR" is present in O, add a new member to O with key "AliasR" and value an
object structure, let it be named Or, and defined as in the following. Otherwise, add all existing members
with "AliasR" to a JSON-LD array and in addition put the object structure Or as defined in the following:

<"object", Robj>.

<"type", "Relationship">.
b)
For each Property associated to Rs (Pss) run the algorithm ALG1.1 taking the following inputs:

Ps → Pss.

O → Or.

C → C.
c)
For each Relationship associated to Rs (Rss) recursively run the present algorithm ALG1.2 taking the
following inputs:

Rs → Rss.

O → Or.

C → C.
2)
Return (O,C) and end of the algorithm.




Annex E (informative):
RDF-compatible specification of NGSI-LD meta-model
The content of this annex is now in ETSI GS CIM 006 [i.8].
