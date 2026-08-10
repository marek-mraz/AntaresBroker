---
clause: C.5.14.3
title: HTTP Response
pages: 403-404
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.14.3 HTTP Response
200 OK
Content-Type: application/ld+json
[
  {
    "id": "urn:ngsi-ld:Vehicle:B9211",
    "type": "Vehicle",
    "speed": {
      "type": "Property",
      "max": [
        [
          120,
          "2018-08-01T12:00:00Z",
          "2018-08-01T12:04:00Z"
        ],
        [
          100,
          "2018-08-01T12:04:00Z",
          "2018-08-01T12:08:00Z"
        ]
      ],
      "avg": [
        [
          120,
          "2018-08-01T12:00:00Z",
          "2018-08-01T12:04:00Z"
        ],
        [
          90,
          "2018-08-01T12:04:00Z",
          "2018-08-01T12:08:00Z"
        ]
      ]
    },
    "@context": [
      "http://example.org/ngsi-ld/latest/vehicle.jsonld",
      "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    ]
  }
]
