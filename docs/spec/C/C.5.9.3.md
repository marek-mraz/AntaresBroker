---
clause: C.5.9.3
title: HTTP Response
pages: 399-400
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.9.3 HTTP Response
200 OK
Content-Type: application/json
Link: <http://example.org/ngsi-ld/latest/aggregatedContext.jsonld>; rel="http://www.w3.org/ns/json-ld#context";
type="application/ld+json"
{
  "id": "http://example.org/vehicle/Vehicle",
  "type": "EntityTypeInfo",
  "typeName": "Vehicle",
  "entityCount": 2,
  "attributeDetails": [
    {
      "id": "http://example.org/vehicle/brandName",
      "type": "Attribute",
      "attributeName": "brandName",
      "attributeTypes": [
        "Property"
      ]
    },
    {
      "id": "http://example.org/vehicle/isParked",
      "type": "Attribute",
      "attributeName": "isParked",
      "attributeTypes": [
        "Relationship"
      ]
    },
    {
      "id": "https://uri.etsi.org/ngsi-ld/location",
      "type": "Attribute",
      "attributeName": "location",
      "attributeTypes": [
        "GeoProperty"
      ]
    },
    {
      "id": "http://example.org/vehicle/speed",
      "type": "Attribute",
      "attributeName": "speed",
      "attributeTypes": [
        "Property"
      ]
    }
  ]
}
