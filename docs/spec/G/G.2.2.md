---
clause: G.2.2
title: Route language sensitive queries via a proxy
pages: '421'
status: informative
evidence: ''
notes: ''
robot: []
---

G.2.2
Route language sensitive queries via a proxy
Create a simple forwarding proxy around the NGSI-LD system. For any urls with a q param (and a collate flag) run a
clean-up of the q param and amend the query string:
The following request on the proxy:
GET /ngsi-ld/v1/entities/?type=Building&q=name==%22Schöne%20Grüße%22&collate=name
is altered on the fly and is sent to the NGSI-LD system as shown:
GET /ngsi-ld/v1/entities/?type=Building&q=name.collate==%22schoene%20gruesse%22
Once again, the substitutions to make to the query string will depend on the rules of the natural language to be
supported.
