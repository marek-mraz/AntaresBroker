---
clause: C.5.8.3
title: HTTP Response
pages: 398-399
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.8.3 HTTP Response
200 OK
Content-Type: application/json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
[
  {
    "id": "http://example.org/vehicle/Vehicle",
    "type": "EntityType",
    "typeName": "Vehicle",
    "attributeNames": [
      "brandName",
      "isParked",
      "location",
      "speed"
    ]
  },
  {
    "id": "http://example.org/parking/OffStreetParking",
    "type": "EntityType",
    "typeName": "OffStreetParking",
    "attributeNames": [
      "availableSpotNumber",
      "isNextToBuilding",
      "location",
      "totalSpotNumber"
    ]
  },
  {
    "id": "http://example.org/parking/ParkingSpot",
    "type": "EntityType",
    "typeName": "http://example.org/parking/ParkingSpot",
    "attributeNames":[
      "location",
      "http://example.org/parking/status"
    ]
  }
]

NOTE:
The type name of all entity types and all attribute names that can be found in the provided @context are
given as short names, the others as Fully Qualified Names (FQNs). The id is always an FQN.
