---
clause: H.5
title: Implementation of the subscription-based actuation workflow
pages: 429-430
status: informative
evidence: ''
notes: ''
robot: []
---

H.5
Implementation of the subscription-based actuation
workflow
The Fed4IoT project (https://fed4iot.org) leverages the NGSI-LD architecture and the subscription/notification
workflow for actuation, in order to implement the concept of a Cloud of Things. It enables virtualization of existing IoT
sensors/actuators through Virtual Things and IoT Brokers. IoT application developers can simply rent the Virtual
Things and the Brokers their applications need.
The Fed4IoT's Cloud of Things is named VirIoT (https://github.com/fed4iot/VirIoT), and it is based on the concept of
Virtual Silos as-a-service: isolated and secure IoT environments made of Virtual Things whose data can be accessed
through standard IoT Brokers (oneM2M, NGSI, NGSI-LD, etc.).
In Figure H.5-1 a diagram shows how VirIoT implements the concept of a large-scale and distribute NGSI-LD system
that leverages the architecture and the workflow convention described in clause H.4.2.
{
"id": "urn:ngsi-ld:pHueActuator:light1",
"type": "Lamp",
"colorRGB": { "type": "Property","value": "0xABABAB"},
"is-on": {"type": "Property","value": true},
"turn-on-STATUS": {"type": "Property","value": "PENDING"}
"turn-on-RESULT": {"type": "Property","value": "OK"}
}
5. Update "is-on"
Property
Context
Consumer
6. Notification
4. and 8.
Notifications
1. Update
"turn-on"
Property
true
true
NGSI-LD system
Context Broker
3. and 7. Update
"turn-on-STATUS" and
"turn-on-RESULT"
Properties
Context Registry
["set-saturation",
"set-hue",
"turn-on", …]
delegated to: "System Adapter"
"turn-on": {
"type": "Property",
"value": true
}
Context Adapter
2a. Is "turn-on"
Property delegated?
Distributed NGSI-LD Entity
2b. Delegated "turn-on" update



Figure H.5-1: VirIoT implementation of the NGSI-LD system and actuation workflow
All components encapsulate requests in a neutral-format message that leverages NGSI-LD Entities at its core. But, since
VirIoT uses MQTT as its internal data distribution system, all information and actuation commands are encoded as
NGSI-LD entities, plus an additional "meta header" that is used by the MQTT to publish and subscribe in a broadcast
fashion to multiple vThings, because the same virtual sensor can be used by multiple applications at the same time.
For the actuation workflow, the "data" part of this message contains the command request, as specified in clause H.3,
but with an additional value key that is the "command notification uri" (cmd-nuri), representing a location where
feedback (status, result) should be sent by the ThingVisor. For example, the cmd-nuri contains the "data_in" MQTT
topic of the issuing vSilo, so that command feedbacks (status and results) are sent to it, only, instead of being
broadcasted to all subscribing applications.
VirIoT is agnostic to access control issues to a virtual actuator, since the relevant policies are implemented in the
specific ThingVisor, which can grant tokens to execute actuation-commands to a subset of vSilos only, through
preliminary exchange of specific actuation-commands (a kind of log-in).
Fed4IoT has developed several different ThingVisors (Context Adapters for different sensors and hardware): for
example, lamp systems and robot devices are virtualized through specific ThingVisors, and applications can control the
lighting system of a rented conference room or control camera and position of a bot by adding related virtual actuators
to their vSilo.
