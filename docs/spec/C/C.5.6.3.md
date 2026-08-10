---
clause: C.5.6.3
title: HTTP Response
pages: '397'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.6.3 HTTP Response
200 OK
Content-Type: application/ld+json
[
  {
    "id": "urn:ngsi-ld:Vehicle:B9211",
    "type": "Vehicle",
    "speed": {
      "type": "Property",
      "values": [
        [
          120,
          "2018-08-01T12:03:00Z"
        ],
        [
          80,
          "2018-08-01T12:05:00Z"
        ],
        [
          100,
          "2018-08-01T12:07:00Z"
        ]
      ]
    },
    "@context": [
      "http://example.org/ngsi-ld/latest/vehicle.jsonld",
      "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    ]
  }
]
