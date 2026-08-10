---
clause: C.5.14.2
title: HTTP Request
pages: '403'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.14.2 HTTP Request
GET /ngsi-
ld/v1/temporal/entities/?type=Vehicle&q=brandName!=Mercedes&attrs=speed&timerel=between&timeAt=2018-
08-01T12:00:00Z&endTimeAt=2018-08-
01T13:00:00Z&aggrMethods=max,avg&aggrPeriodDuration=PT4M&format=aggregatedValues
Accept: application/ld+json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
