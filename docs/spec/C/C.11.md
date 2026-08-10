---
clause: C.11
title: Entity with digital signature for a Property
pages: 412-414
status: informative
evidence: ''
notes: ''
robot: []
---

C.11 Entity with digital signature for a Property
As specified in [35], the atomic piece of information that creators can digitally sign in an NGSI-LD ecosystem is each
single Attribute of an Entity. In the following example, an Entity of type "Store" with two Properties, "address" and
"location" is presented. The "address" Property is digitally signed. The signature is created using one Ed25519
instantiation of the Edwards-Curve Digital Signature Algorithm (EdDSA).The used crypto suite is "eddsa-rdfc-2022".


EXAMPLE:
Entity of type "Store" with two Properties. The "address" Property is digitally signed.
{
  "id": "urn:ngsi-ld:Store:002",
  "type": "Store",
  "address": {
    "type": "Property",
    "value": {
      "streetAddress": ["Tiger Street 4", "al"],
      "addressRegion": "Metropolis",
      "addressLocality": "Cat City",
      "postalCode": "42420"
    }
    "ngsildproof": {
      "type": "Property",
      "entityIdSealed": "urn:ngsi-ld:Store:002",
      "entityTypeSealed": "Store",
      "value": {
        "type": "DataIntegrityProof",
        "created": "2025-01-27T21:02:24Z",
        "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT",
        "cryptosuite": "eddsa-rdfc-2022",
        "proofPurpose": "assertionMethod",
        "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"
      }
    }
  },
  "location": {
    "type": "GeoProperty",
    "value": {
      "type": "Point",
      "coordinates": [57.5522, -20.3484]
    }
  },
  "@context": "https://uri.etsi.org/ngsi-ld/primer/store-context.jsonld"
}





Annex D (informative):
Transformation Algorithms
D.1
Introduction
These algorithms are informative but NGSI-LD implementations should aim at either implementing them as they are
described here or devising similar algorithms which take exactly the same input and provides exactly the same output
(or an equivalent one as per the JSON-LD specification [2]).
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
