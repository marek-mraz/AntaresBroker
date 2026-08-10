---
clause: G.1.3
title: Associating as equivalent entity
pages: '420'
status: informative
evidence: ''
notes: ''
robot: []
---

G.1.3
Associating as equivalent entity
Where equivalent context entities in multiple natural languages exist, they may be associated with each other through
the use of a one-to-many relationship, where each relationship holds an additional sub-Property indicating the natural
language of the equivalent entities.
For example, three Events (such as a walking tour which is available in English, French and German) may be associated
to each other as follows:
{
    "type": "Event",
    "id": "urn:ngsi-ld:Event:bonjourLeMonde",
    "name": {
        "type": "Property",
        "value": "Bonjour le Monde"
    },
    "sameAs": [
        {
            "type": "Relationship",
            "datasetId" : "urn:ngsi-ld:Relationship:1",
            "object": "urn:ngsi-ld:Event:helloWorld",
            "inLanguage": {
                    "type": "Property",
                    "value": "en"
            }
        },
        {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Event:halloWelt",
            "inLanguage": {
                    "type": "Property",
                    "value": "de"
            }
        }
    ]
 }
