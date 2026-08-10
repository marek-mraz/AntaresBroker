---
clause: C.2.4
title: '@context'
pages: 391-392
status: informative
evidence: ''
notes: ''
robot: []
---

C.2.4
@context
The disposition of the @context can be as an inline JSON object, as a dereferenceable URI or as a (multiple)
combination of both. In the examples above the @context is provided through several dereferenceable URIs. The
resulting @context (obtained by merging the content of the resource referenced by the referred URIs) is shown below.
NOTE 1: For brevity reasons the @context does not contain the API terms defined by clause 5.2.
NOTE 2: Some extra terms are defined because they will be used in examples later presented.
{

"id": "@id",

"type": "@type",

"Property": "https://uri.etsi.org/ngsi-ld/Property",

"Relationship": "https://uri.etsi.org/ngsi-ld/Relationship",

"value": "https://uri.etsi.org/ngsi-ld/hasValue",

"object": {


"@type": "@id",


"@id": "https://uri.etsi.org/ngsi-ld/hasObject"

},

"observedAt": {


"@type": "https://uri.etsi.org/ngsi-ld/DateTime",


"@id": "https://uri.etsi.org/ngsi-ld/observedAt"

},

"datasetId": {


"@id": "https://uri.etsi.org/ngsi-ld/datasetId",


"@type": "@id"

},

"location": "https://uri.etsi.org/ngsi-ld/location",

"GeoProperty": "https://uri.etsi.org/ngsi-ld/GeoProperty",

"Vehicle": "http://example.org/vehicle/Vehicle",

"street": "http://example.org/vehicle/street",

"brandName": "http://example.org/vehicle/brandName",

"category": "http://example.org/vehicle/category",

"tyreTreadDepths": "http://example.org/vehicle/tyreTreadDepths",

"passengers": "http://example.org/vehicle/passengers",

"speed": "http://example.org/vehicle/speed",

"isParked": {


"@type": "@id",


"@id": "http://example.org/common/isParked"

},

"OffStreetParking": "http://example.org/parking/OffStreetParking",

"availableSpotNumber": "http://example.org/parking/availableSpotNumber",

"totalSpotNumber": "http://example.org/parking/totalSpotNumber",

"isNextToBuilding": {


"@type": "@id",


"@id": "http://example.org/common/isNextToBuilding"

},

"reliability": "http://example.org/common/reliability",

"providedBy": {


"@type": "@id",


"@id": "http://example.org/common/providedBy"

},

"name": "http://example.org/common/name",

"commercial": "http://example.org/vehicle/commercial",

"non-commercial": "http://example.org/vehicle/non-commercial",

"Integer": "http://www.w3.org/2001/XMLSchema#Integer
}
