---
clause: C.5.15.3
title: HTTP Response
pages: 404-405
status: informative
evidence: ''
notes: ''
robot: []
---

C.5.15.3 HTTP Response
200 OK
Content-Type: application/ld+json
[
   {
      "id": "urn:ngsi-ld:OffStreetParking:Downtown1",
      "type": "OffStreetParking",
      "scope": "/Madrid/Centro",
      "name": {
         "type": "Property",
         "value": "Downtown One"
      },
      "availableSpotNumber": {
         "type": "Property",
         "value": 121,
         "observedAt": "2017-07-29T12:05:02Z",
         "reliability": {
            "type": "Property",
            "value": 0.7
         },
         "providedBy": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Camera:C1"
         }
      },
      "totalSpotNumber": {
         "type": "Property",
         "value": 200
      },
      "location": {
         "type": "GeoProperty",
         "value": {
            "type": "Point",
            "coordinates": [
               -8.5,
               41.2
            ]
         }
      },
      "@context": [
         "http://example.org/ngsi-ld/latest/parking.jsonld",
         "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
      ]
   },
   {
      "id": "urn:ngsi-ld:OffStreetParking:Corte4",
      "type": "OffStreetParking",
      "scope": [
            "/Madrid/Cortes",
            "/Company894/UnitC"
      ],
      "name": {


         "type": "Property",
         "value": "Corte4"
      },
      "availableSpotNumber": {
         "type": "Property",
         "value": 121,
         "observedAt": "2017-07-29T12:05:02Z",
         "reliability": {
            "type": "Property",
            "value": 0.7
         },
         "providedBy": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Camera:C1"
         }
      },
      "totalSpotNumber": {
         "type": "Property",
         "value": 100
      },
      "location": {
         "type": "GeoProperty",
         "value": {
            "type": "Point",
            "coordinates": [
               -8.6,
               41.3
            ]
         }
      },
      "@context": [
         "http://example.org/ngsi-ld/latest/parking.jsonld",
         "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
      ]
   }
]
