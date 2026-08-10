---
clause: C.5.10.3
title: HTTP Response
pages: '400'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.10.3 HTTP Response
200 OK
Content-Type: application/json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
{
  "id": "urn:ngsi-ld:AttributeList:56534657",
  "type": "AttributeList",
  "attributeList": [
    "brandName",
    "isParked",
    "location",
    "speed",
    "http://example.org/parking/status"
  ]
}

NOTE:
The attribute names that can be found in the provided @context are given as short names, the others as
Fully Qualified Names (FQNs).
