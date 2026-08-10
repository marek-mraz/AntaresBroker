---
clause: G.1.2
title: Associating a Property with a Natural Language
pages: 419-420
status: informative
evidence: ''
notes: ''
robot: []
---

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


    },

    "bookingUrl": {
        "type": "Property",
        "value": [
            "http://example.com/booking-in-french/",
            "http://example.com/booking-in-english/",
            "http://example.com/booking-in-german/"
        ],
        "inLanguage": {
            "type": "Property",
            "value": ["fr", "en", "de" ]
        }
    }
}
