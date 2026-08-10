---
clause: G.3.0
title: Foreword
pages: '421'
status: informative
evidence: ''
notes: ''
robot: []
---

G.3.0
Foreword
Context data entities are designed to be interoperable and therefore all dates are held as UTC dates, all currency
amounts are held as JSON numbers (with the unitCode property-of-a-property available to hold the currency), etc.
Localization should not occur within the context data entities themselves. Offering fully localized responses is not a
concern of the NGSI-LD API.
If localization support is necessary, a simple proxying a conversion mechanism could be used to amend the context data
received from the NGSI-LD system before being passed to a third party system for display.
