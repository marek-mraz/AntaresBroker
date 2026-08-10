---
clause: C.5.12.3
title: HTTP Response
pages: '402'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.12.3 HTTP Response
200 OK
Content-Type: application/json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
{
  "id": "http://example.org/vehicle/brandName",
  "type": "Attribute",
  "attributeName": "brandName",
  "attributeTypes": ["Property"],
  "typeNames": ["Vehicle"],
  "attributeCount": 2
}
