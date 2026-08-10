---
clause: C.5.4.3
title: HTTP Response
pages: 394-395
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.4.3 HTTP Response
200 OK
Content-Type: application/ld+json
Link: </ngsi-ld/v1/entities/?type= Vehicle&format=simplified&limit=2&offset=2>; rel="next";
type="application/ld+json"
[
  {
    "id": "urn:ngsi-ld:Vehicle:B9211",
    "type": "Vehicle",
    "brandName": "Volvo",
    "@context": [
      "http://example.org/ngsi-ld/latest/vehicle.jsonld",
      "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    ]
  },
  {
    "id": "urn:ngsi-ld:Vehicle:A456",
    "type": "Vehicle",
    "brandName": "Mercedes",


    "@context": [
      "http://example.org/ngsi-ld/latest/vehicle.jsonld",
      "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    ]
  }
]
