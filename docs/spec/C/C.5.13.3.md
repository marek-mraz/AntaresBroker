---
clause: C.5.13.3
title: HTTP Response
pages: 402-403
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.13.3 HTTP Response
200 OK
Content-Type: application/ld+json
[
  {
    "id": "urn:ngsi-ld:Vehicle:A4567",
    "type": "Vehicle",
    "marque": "Opel Karl",
    "@context": [
      "http://example.org/ngsi-ld/latest/vehicle.jsonld",
      "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    ]
  }
]
