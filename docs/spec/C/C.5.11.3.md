---
clause: C.5.11.3
title: HTTP Response
pages: '401'
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.11.3 HTTP Response
200 OK
Content-Type: application/json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
[
  {
    "id": "http://example.org/vehicle/brandName",
    "type": "Attribute",
    "attributeName": "brandName",
    "typeNames": [
      "Vehicle"
    ]
  },
  {
    "id": "http://example.org/vehicle/isParked",
    "type": "Attribute",
    "attributeName": "isParked",
    "typeNames": [
      "Vehicle"
    ]
  },
  {
    "id": "https://uri.etsi.org/ngsi-ld/location",
    "type": "Attribute",
    "attributeName": "location",
    "typeNames": [
      "Vehicle",
      "OffStreetParking",
      "http://example.org/parking/ParkingSpot"
    ]
  },
  {
    "id": "http://example.org/vehicle/speed",
    "type": "Attribute",
    "attributeName": "speed",
    "typeNames": [
      "Vehicle"
    ]
  },
  {
    "id": "http://example.org/parking/status",
    "type": "Attribute",
    "attributeName": "http://example.org/parking/status",
    "typeNames": [
      "http://example.org/parking/ParkingSpot"
    ]
  }
]

NOTE:
The attribute name and all type names that can be found in the provided @context are given as short
names, the others as Fully Qualified Names (FQNs). The id is always an FQN.
