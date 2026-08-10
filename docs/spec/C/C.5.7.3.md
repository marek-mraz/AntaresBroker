---
clause: C.5.7.3
title: HTTP Response
pages: 397-398
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.7.3 HTTP Response
200 OK
Content-Type: application/json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
{
  "id": "urn:ngsi-ld:EntityTypeList:34534657",
  "type": "EntityTypeList",
  "typeList": [
    "Vehicle",
    "OffStreetParking",
    "http://example.org/parking/ParkingSpot"
  ]
}



NOTE:
All entity types that can be found in the provided @context are given as short names, the others as Fully
Qualified Names (FQNs).
