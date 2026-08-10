---
clause: A.3
title: NGSI-LD namespace
pages: 364-365
status: not-implemented
evidence: ''
notes: ''
robot: []
---

A.3
NGSI-LD namespace
NGSI-LD defines a specific URN [9] namespace intended to help API users to design readable, clean and simple
identifiers. As it is based on URNs, the usage of this identification approach is not recommended when dereferenceable
URIs are needed (fully-fledged linked data scenarios).
The referred namespace is defined as follows (to be registered with IANA):
•
namespace identifier: NID = "ngsi-ld"
•
namespace specific string: NSS = EntityTypeName ":" EntityIdentificationString
EntityTypeName shall be an Entity Type name which can be expanded to a URI as per the @context.
EntityIdentificationString shall be a string that allows uniquely identifying the subject Entity in combination with the
other items being part of the NSS.
EXAMPLE:
urn:ngsi-ld:Person:28976543.
It is recommended that applications use this URN namespace when applicable.
In general, the URN specification defines namespace equivalence in a case-insensitive manner, however it is assumed
that context-broker implementations shall always use lowercase letters in namespaces where they have a choice in case,
unless there is a strong reason otherwise. Restricting the namespace prefix to lower case urn:ngsi-ld: can improve
caching and retrieval, since this ensures since alphabetic characters within the namespace specific string are always
consistent.



Annex B (normative):
Core NGSI-LD @context definition
Below is the definition of the Core NGSI-LD @context which shall be supported by implementations.
Such definition has been tested using [i.7].
{
  "@context": {
    "@version": 1.1,
    "@protected": true,
    "ngsi-ld": "https://uri.etsi.org/ngsi-ld/",
    "geojson": "https://purl.org/geojson/vocab#",
    "id": "@id",
    "type": "@type",
    "Attribute": "ngsi-ld:Attribute",
    "AttributeList": "ngsi-ld:AttributeList",
    "ContextSourceIdentity": "ngsi-ld:ContextSourceIdentity",
    "ContextSourceNotification": "ngsi-ld:ContextSourceNotification",
    "ContextSourceRegistration": "ngsi-ld:ContextSourceRegistration",
    "Date": "ngsi-ld:Date",
    "DateTime": "ngsi-ld:DateTime",
    "EntityType": "ngsi-ld:EntityType",
    "EntityTypeInfo": "ngsi-ld:EntityTypeInfo",
    "EntityTypeList": "ngsi-ld:EntityTypeList",
    "ExecutionResultDetails": "ngsi-ld:ExecutionResultDetails",
    "Feature": "geojson:Feature",
    "FeatureCollection": "geojson:FeatureCollection",
    "GeoProperty": "ngsi-ld:GeoProperty",
    "GeometryCollection": "geojson:GeometryCollection",
    "JsonProperty": "ngsi-ld:JsonProperty",
    "LanguageProperty": "ngsi-ld:LanguageProperty",
    "LineString": "geojson:LineString",
    "ListProperty": "ngsi-ld:ListProperty",
    "ListRelationship": "ngsi-ld:ListRelationship",
    "MultiLineString": "geojson:MultiLineString",
    "MultiPoint": "geojson:MultiPoint",
    "MultiPolygon": "geojson:MultiPolygon",
    "Notification": "ngsi-ld:Notification",
    "Point": "geojson:Point",
    "Polygon": "geojson:Polygon",
    "Property": "ngsi-ld:Property",
    "Relationship": "ngsi-ld:Relationship",
    "Snapshot": "ngsi-ld:Snapshot",
    "SnapshotNotification": "ngsi-ld:SnapshotNotification",
    "Subscription": "ngsi-ld:Subscription",
    "TemporalProperty": "ngsi-ld:TemporalProperty",
    "Time": "ngsi-ld:Time",
    "VocabProperty": "ngsi-ld:VocabProperty",
    "accept": "ngsi-ld:accept",
    "aggrParams": "ngsi-ld:aggrParams",
    "aggrMethods": "ngsi-ld:aggrMethods",
    "aggrPeriodDuration": "ngsi-ld:aggrPeriodDuration",
    "attributeCount": "attributeCount",
    "attributeDetails": "attributeDetails",
    "attributeList": {
      "@id": "ngsi-ld:attributeList",
      "@type": "@vocab"
    },
    "attributeName": {
      "@id": "ngsi-ld:attributeName",
      "@type": "@vocab"
    },
    "attributeNames": {
      "@id": "ngsi-ld:attributeNames",
      "@type": "@vocab"
    },
    "attributeTypes": {
      "@id": "ngsi-ld:attributeTypes",
      "@type": "@vocab"
    },
    "attributes": {
      "@id": "ngsi-ld:attributes",
      "@type": "@vocab"
    },
