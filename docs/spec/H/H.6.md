---
clause: H.6
title: Implementation of the registration-based actuation workflow
pages: 430-433
status: informative
evidence: ''
notes: ''
robot: []
---

H.6
Implementation of the registration-based actuation
workflow
The IoT Agent node library [i.22] introduces the concept of an IoT Agent, which is a component that lets a group of
devices send their data to and be managed from a Context Broker using their own native protocols. Thus, it
corresponds to the Context Adapter, and wires up the IoT devices so that measurements can be read and commands can
be sent using NGSI-LD requests sent to an NGSI-LD compliant context Context Broker.
IoT Agents already exist or are in development for many IoT communication protocols and data models. Examples
include the following:
•
IoTAgent-JSON - a bridge between HTTP/MQTT messaging (with a JSON payload) and NGSI-LD
•
IoTAgent-LWM2M - a bridge between the Lightweight M2M protocol and NGSI-LD
•
IoTAgent-UL - a bridge between HTTP/MQTT messaging (with an UltraLight2.0 payload) and NGSI-LD
•
IoTagent-LoRaWAN - a bridge between the LoRaWAN protocol and NGSI-LD
Actuator
ThingVisor
(vThing)
IoT vSilo
Controller
IoT vSilo
Broker
MQTT
Distribution
System
vSilo
Conversion to/from NGSI-LD + MQTT metadata
Context
Consumer
Large-scale distributed NGSI-LD system
Context Broker
Context Adapter
API


This implementation follows the communication model described in clause H.4.3, as explained in Figure H.6-1. In this
workflow:
•
Requests between User and Context Broker use NGSI-LD
•
Requests between Context Broker and IoT Agent use NGSI-LD
•
Requests between IoT Agent and IoT Device use native protocols
•
Requests between IoT Device and IoT Agent use native protocols
•
Requests between IoT Agent and Context Broker use NGSI-LD

Figure H.6-1: IoT Agent node library implementation of the NGSI-LD system and actuation workflow


Provisioning of the devices will be carried out (via REST API) through IoT Agents, as well. This provisioning implies
that, on the one hand, the corresponding entities (with their commands), that represent the devices, are generated in the
Context Broker and, on the other hand, that the corresponding IoT Agent is configured for communication with
the corresponding device, all in one provisioning step. Below, an example how to provision a device which supports
start and stop commands is presented.
{
    "devices": [
        {
            "device_id":   "device001",
            "entity_name": "urn:ngsi-ld:Device:001",
            "entity_type": "Device",
            "attributes": [
                { "object_id": "s", "name": "isOpen", "type": "boolean" }
            ],
            "commands": [
                { "name": "start", "type": "command" },
                { "name": "stop", "type": "command" }
            ],
            "static_attributes": [
                { "name":
                    { "type": "Text", "value": "Device:001 provision" }
                }
            ]
        }
    ]
}





Annex I (informative):
Change history
Date
Version
Information about changes
February 2020
1.2.10
Early draft copied from API version 1.2.1
February 2020
1.2.11
Unicode characters. Query Language syntax changes to Attribute path, and extension
to accept specifying just Query or Geoquery when Querying Entities
Acknowledgements to EU projects. Lightweight Figures
March 2020
1.2.12
Extending to other interactions the above changes to query entities interaction
Changes to ABNF Query Language syntax to access complex objects value of
properties more easily
Generalized Notification Headers, in order to carry authentication etc., info
Novel &option=count and associated Header to indicate number of Entities in
response to a query
Novel/entityOperations/query and/temporal/entityOperations/query endpoints to
perform query via POST
Clarified attrs URL parameter behaviour
Support for Multiple Attributes
Support for Multiple Tenants
May 2020
Candidate
1.2.13
from 101r1: Multi-Attribute-Support-fix-in-4.5.5
from 102r1: Batch_Operation_Error_Codes
from 110r1: JSON-LD Validation clause
from 112r1: IRI Support for International Characters
from 115r2: More Core Context Changes
from 130: Entity Types
MQTT Notifications
GeoJSON Representation
9 July 2020
1.3.1
Technical Officer verifications for submission to editHelp! publication pre-processing
August 2020
1.3.2
New baseline towards v1.4.1
November 2020
1.3.3
From 272r1: Support for natural languages via LanguageProperty; annex G
December 2020
1.3.4
From 319: Align Table 6.8.3.2-1 with clause 5.10.2 for query via attrs
December 2020
1.3.4
From 310r2: Dot vs. comma in DateTime
December 2020
1.3.4
From 309r1: Remove sentences referring to old multi attributes representation
December 2020
1.3.4
From 308r: id and type for JSON-LD compliance
December 2020
1.3.4
From 313r1: FIXES to Cross domain data model for LanguageProperties
Bug fixes and errata
December 2020
1.3.5
From 275r3: Temporal Aggregation Functions
December 2020
1.4.0
1.3.5 with small typos corrected, approved as 1.4.0
January 2021
1.4.1
ETSI Technical Officer review for ETSI EditHelp publication pre-processing
March 2021
1.4.2
Editorial Changes, clarifications added, better references, figures replacements and
corrections, figures merged, typos, code indentation
April 2021
1.4.2
Temporal Pagination
April 2021
1.4.2
Clarified behaviour when multiple instances of the same Entity are in an input array
July 2021
1.4.3
From 130r6: NGSI-LD Scope
July 2021
1.4.3
From 143r6: Storing, managing and serving @contexts
July 2021
1.4.3
From 120r4: API structuring
October 2021
1.4.4
From 156: Remove static elements from temporal representations
October 2021
1.4.4
From 155: Existence query
October 2021
1.4.4
From 152: Remove null value deletion
October 2021
1.4.4
From 151: attrs missing in core context
October 2021
1.5.1
ETSI Technical Officer review for ETSI EditHelp publication pre-processing
November 2021
1.5.2
First early draft after EditHelp publication of V1.5.1 to prepare next V1.6.1 publication
January 2022
1.5.3
Concise representation
May 2022
1.5.4
PUT/PATCH Entity
May 2022
1.5.4
Distributed operations
July 2022
1.5.5
From 99r6: Deletions and advanced notifications
July 2022
1.5.5
From 106r1: Workflow for actuation
July 2022
1.5.5
From 105r1: Context Source Info in Context Source Registration
July 2022
1.5.5
From 93r1: default context clarification
July 2022
1.5.5
From 91r1: CR_on_Scope_ABNF_format
Juy 2022
1.6.1
Final Technical Official check for EditHelp publication
