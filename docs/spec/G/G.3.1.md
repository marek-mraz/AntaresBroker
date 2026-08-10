---
clause: G.3.1
title: Localizing Dates
pages: 421-423
status: informative
evidence: ''
notes: ''
robot: []
---

G.3.1
Localizing Dates
For example, if a system needs to display DateTime data in Islamic Date format
The following request on the proxy:
GET /ngsi-ld/v1/entities/urn:ngsi-ld:Event:XXX?attrs=date&format=simplified
is forwarded unaltered and is sent to the NGSI-LD system as shown:
GET /ngsi-ld/v1/entities/urn:ngsi-ld:Event:XXX?attrs=date&format=simplified
The response from the NGSI-LD system is always in UTC format:

{"date": "2020-09-28T17:13:39+02:00"}



And the proxy can be used to update this to the desired format:

{"date": "11 Safar, 1442 1:13:39PM"}

Using an internationalization script such as the following:
new Intl.DateTimeFormat("en-u-ca-islamic", {day: 'numeric', month: 'long',weekday: 'long',year :
'numeric'}).format(date);

It should be noted that post-localization, the transformed date is no longer valid NGSI-LD.




Annex H (informative):
Suggested actuation workflows
H.1
Actuators and feedback to the consumer
Actuators are things that can change their state (light on/off) or execute actions (move forward, detect face, etc.).
There is currently no explicitly and precisely specified support for actuation in the NGSI-LD API. Thus, this clause
describes some conventions that represent a proposed best-practice about how NGSI-LD API and data models can be
used for the interaction between applications and actuators represented by NGSI-LD Entities.
The conventions and approach described in this clause are not powerful enough to implement complex actuation jobs
that depend on each other and, for instance, make actuation decisions conditional on the outcome of other actuations,
unless that behaviour is implemented in a custom way within the application logic. The concept of a more evolved
service execution logic, being a first-class citizen of the NGSI-LD API and able to offer more structured building blocks
for actuation, is outside the scope of this annex.
An NGSI-LD system that comprises an actuator and supports actuation workflows is represented as one or more
NGSI-LD Entities, plus a Context Broker, a Context Source or a Context Producer, and a Context
Consumer, which collaborate.
The interaction between actuator and Context Consumer needs to be bidirectional. Thus, actuators are triggered by
the reception of actuation-specific commands (e.g. "set the on state of the lamp to false", to turn the light off) that are
encoded as NGSI-LD data, following a suggested data model. They respond with feedback, similarly encoded as
NGSI-LD data.
Command feedbacks may serve to control the maximum operations rate a controlling application needs to achieve, and
different levels of feedback can be requested, by associating a specific Quality of Service value to the command:
•
Some applications need high operation rate but no feedback. For this case a QoS = 0 can be used. The typical
example is to control the arms of a robot with a joystick.
•
Some applications need to be sure that the actuators actually received the command request or need to get back
a payload in response to the command. In this case a QoS = 1 can be used. The typical case is switching on a
light with confirmation, or request face-detection with consequent notification of matching events.
•
Commands can either require a short or a long execution time. For commands with long execution time, the
application may require a continuous status feedback. In this case a QoS = 2 can be used. The typical example
is that of a door opening, where feedback continuously reports the current level (10 % open, 50 % open, etc.).
H.2
Architecture for actuation
In this architecture, the application acts as Context Consumer, and the terms are used interchangeably.
Commands are sent to the Context Broker by the Context Consumer, using the standard NGSI-LD API and a
suggested convention for representing them. In turn, feedback about command execution is received by the Context
Consumer, both as continuous status updates and/or a final command result.
More specifically, the component that handles direct communication with the actuator is the Context Source or the
Context Producer: it uses an actuator-specific protocol to control the actuator and get responses and updates from
it, i.e. from the real system. Such Context Source/Consumer or Context Producer/Storage acting as a proxy
or intermediary to the actuator is referred to, in the following, as Context Adapter.
Thus, the Context Adapter is able to use the NGSI-LD API to receive NGSI-LD command requests from the NGSI-LD
Context Broker and send back command status and result to it, as well as using an actuator-specific protocol to
communicate with the actuator.
The NGSI-LD Context Broker is responsible for handling direct communication with the Context Consumer.
