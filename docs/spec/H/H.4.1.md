---
clause: H.4.1
title: Possible communication models
pages: '427'
status: informative
evidence: ''
notes: ''
robot: []
---

H.4.1
Possible communication models
This convention can be leveraged by two different communication models:
•
Subscription/notification, where both the application and the Context Adapter use NGSI-LD Subscriptions to
have the command requests delivered to the appropriate handler within the Context Adapter and vice-versa. In
this case the Context Adapter acts as a Context Source as well as a Context Consumer.
•
Forwarding, which uses the NGSI-LD Registry and a Context Adapter able to federate itself with the
Context Broker holding the actuator's Entity, as a means to deliver the commands. In this case the
Context Adapter acts as a Context Storage as well as a Context Producer.
