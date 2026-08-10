---
clause: D.2
title: Algorithm for transforming an NGSI-LD Entity into a JSON-LD document (ALG1)
pages: 414-415
status: informative
evidence: ''
notes: ''
robot: []
---

D.2
Algorithm for transforming an NGSI-LD Entity into a
JSON-LD document (ALG1)
This algorithm takes as input an NGSI-LD graph which top level node is a particular Entity and returns as output a
JSON-LD document which represents all the data associated to the entity. The JSON-LD document (and its associated
@context) corresponds to a representation of the Entity in JSON-LD as per the NGSI-LD Information Model.
NOTE:
An early implementation of this algorithm can be found at [i.5].
Let:
•
G be a graph defined as follows:
-
Let N be G's top level node.
-
N is an Entity instance of type T or types Ts. Type name is "AliasT" or there is an Array of Type names
["AliasT1", …,"AliasTn"], N's identifier is I.
-
N has 0 or more associated Property. Each Property (Psi) is defined as follows:

Property type identifier is Pi.

Property name is "AliasPi".

Property Value is Vi.

Property Value's associated data type is Di.
-
N is the subject of 0 or more Relationship. Each Relationship is defined as follows:

Relationship type identifier is Ri.

Relationship name is "AliasRi".

Relationship target object identifier is Robji.
•
O be a JSON object initialized to the empty object ({}).
•
C be a JSON-LD @context initialized as described by annex B.
The algorithm should run as follows, provided all the preconditions defined above are satisfied:
1)
Add to C a new member <"AliasT", T> or new members <"AliasT1", T1> … <"AliasTn", Tn>.
2)
Add to O two new members:
a)
<"id", I>.
b)
<"type", "AliasT"> or <"type", ["AliasT1", …,"AliasTn"]> .">.


3)
For each Property Psi (Pi, "AliasP", Vi, Di) associated to N:
a)
Run Algorithm ALG1.1 taking the following inputs:

Ps → Psi.

O → O.

C → C.
4)
For each Relationship Rs (Ri, AliasRi, Robji) associated to N:
a)
Run Algorithm ALG1.2 taking the following inputs:

Rs → Rsi.

O → O.

C → C.
5)
Return (O, C) and end of the algorithm.
