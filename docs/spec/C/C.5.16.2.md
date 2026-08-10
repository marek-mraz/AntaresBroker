---
clause: C.5.16.2
title: HTTP Request
pages: '405'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.16.2 HTTP Request
GET /ngsi-ld/v1/temporal/entities/?type=Vehicle&attrs=speed,scope&timerel=between&timeAt=2018-08-
01T12:00:00Z&endTimeAt=2018-08-01T13:00:00Z&scopeQ="/Madrid/Centro"
Accept: application/ld+json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
