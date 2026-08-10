---
clause: C.2.1
title: Property Graph
pages: 371-372
status: informative
evidence: ''
notes: ''
robot: []
---

C.2.1
Property Graph
Figure C.2.1-1 shows a diagram representing a property graph to be used for the examples discussed in this clause.

Figure C.2.1-1: Reference example
As per the algorithms described above and as per the rules for generating the JSON-LD representation of NGSI-LD
entities the above graph will result in the following JSON-LD representations. The syntax has been checked using the
JSON-LD Playground tool [i.5].
Vehicle
urn:ngsi-ld:
Vehicle:
A4567
"Mercedes"
2017-07-
29T12:00:04Z
urn:ngsi-ld:
OffStreetParking:
Downtown1
urn:ngsi-ld:
Person:
Bob
Person
OffStreetParking
urn:ngsi-ld:
Camera:C1
Camera
0.7
brandName
parkingDate
reliability
availableSpot
Number
isParked
provided
By
provided
By
Property
Relationship
Entity
Entity Type
type
hasValue
hasObject
