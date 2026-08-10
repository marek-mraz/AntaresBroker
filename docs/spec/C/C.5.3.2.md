---
clause: C.5.3.2
title: HTTP Request
pages: '394'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.3.2 HTTP Request
GET /ngsi-ld/v1/entities/?type=Vehicle&q=brandName!="Mercedes"&format=simplified
Accept: application/ld+json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
