---
clause: G.2.1
title: Maintain collations as metadata
pages: '421'
status: informative
evidence: ''
notes: ''
robot: []
---

G.2.1
Maintain collations as metadata
•
Create a subscription on the attribute (e.g. name)
•
Create a simple microservice to add/upsert a name.collate property-of-a-property using a simple function to
strip all diacritic marks - for example:
str.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLower()
Other substitutions could be made where local spelling rules vary (for example different for German ö = oe).
