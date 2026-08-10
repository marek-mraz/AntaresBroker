---
clause: C.5.5.4
title: 'HTTP Request #2'
pages: '396'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.5.4 HTTP Request #2
GET /ngsi-
ld/v1/temporal/entities/?type=Vehicle&q=brandName!=Mercedes&attrs=speed,brandName&timerel=between&tim
eAt=2018-08-01T12:00:00Z&endTimeAt=2018-08-01T13:00:00Z
Accept: application/ld+json
Link: <http://example.org/ ngsi-ld /latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
