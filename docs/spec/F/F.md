---
clause: F
title: 'Annex F (informative): Conventions and syntax guidelines'
pages: 418-419
status: informative
evidence: ''
notes: ''
robot: []
---

Annex F (informative):
Conventions and syntax guidelines
When new terms are defined, they are marked in bold, and terms are capitalized thereafter.
EXAMPLE 1:
NGSI-LD Linked Entity, Linked Entity.
API Parameter names are always in lowercase.
EXAMPLE 2:
options.
Entity Types are defined using lowercase but with a starting capital letter.
EXAMPLE 3:
Vehicle, Building, ParkingSpace.
JSON-LD nodes and terms are always defined using camel case notation starting with lower case.
EXAMPLE 4:
createdAt, value, unitCode.
When referring to special terms, data types or words defined previously in the present document or by other referenced
specifications, italics format is used.
EXAMPLE 5:
ListRelationship, GeoProperty, Geometry, Second, Number.
When referring to literal strings double quotes are used.
EXAMPLE 6:
"application/json", "Subscription".
When referring to the JSON-LD Context the mnemonic text string @context is used as a placeholder.
All the dates and times are given in UTC format.
EXAMPLE 7:
2018-02-09T11:00:00Z.
The measurement units used in the API are those defined by the International System of Units.
EXAMPLE 8:
The distance in geo-queries is provided in meters.
When defining application-specific elements or API extensions the same conventions and syntax guidelines should be
followed.




Annex G (informative):
Localization and Internationalization Support
G.0
Foreword
These algorithms described below are informative, but NGSI-LD implementations should aim at either implementing
them as they are described here or providing equivalent @context elements for their payloads to provide interoperability
with their internationalized context entities.
G.1
Introduction
G.1.0
Foreword
Since Internationalization is not core to context information management, any direct support within NGSI-LD systems
is limited. Annex G proposes a series of best practices for maintaining, querying and displaying interoperable
internationalized data.
The content of the @context utilized for the referred Entities within these examples uses pre-existing URNs used for
internationalization and is as follows:
{
"inLanguage": "http://schema.org/inLanguage",
"sameAs": "http://schema.org/sameAs"
}

G.1.1
Associating an Entity with a Natural Language
Where a context Entity is associated with a single natural language, include a well-defined Property indicating the
natural language of the content. For example an Event taking place in French may be defined as follows:
{
    "type": "Event",
    "id": "urn:ngsi-ld:Event:bonjourLeMonde",
    "name": {
        "type": "Property",
        "value": "Bonjour le Monde"
    },
    "description": {
         "type": "Property",
         "value": "«Bonjour le monde» sont les mots traditionnellement écrits par un programme
informatique simple"
    },
    "inLanguage": {
        "type": "Property",
        "value": "fr"
    }
}

G.1.2
Associating a Property with a Natural Language
Where a Property of a context entity can be associated to one more natural language, include additional metadata as a
sub-Property of that Property. For example, a Hotel with booking forms available in English, French and German may
be defined as follows:
{
    "type": "Hotel,
    "id": "urn:ngsi-ld:Hotel:XXXXX",
    "name": {
        "type": "Property",
        "value": "Grand Hotel"
