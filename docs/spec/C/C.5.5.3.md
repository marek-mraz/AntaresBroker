---
clause: C.5.5.3
title: 'HTTP Response #1'
pages: 395-396
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.5.3 HTTP Response #1
200 OK
Content-Type: application/ld+json
[
  {
    "id": "urn:ngsi-ld:Vehicle:B9211",
    "type": "Vehicle",
    "speed": [
      {
        "type": "Property",
        "value": 120,
        "observedAt": "2018-08-01T12:03:00Z"
      },
      {
        "type": "Property",
        "value": 80,
        "observedAt": "2018-08-01T12:05:00Z"
      },
      {
        "type": "Property",
        "value": 100,
        "observedAt": "2018-08-01T12:07:00Z"
      }
    ],
    "@context": [
      "http://example.org/ngsi-ld/latest/vehicle.jsonld",
      "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    ]
  }
]
