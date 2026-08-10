---
clause: G.1.1
title: Associating an Entity with a Natural Language
pages: '419'
status: informative
evidence: ''
notes: ''
robot: []
---

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
