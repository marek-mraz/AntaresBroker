---
clause: I
title: 'Annex I (informative): Change history'
pages: 433-436
status: informative
evidence: ''
notes: ''
robot: []
---

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


Date
Version
Information about changes
October 2022
1.6.2
New early draft:
corrected Annex C6 date representation
from 149r3: generalized description of @context in bullet lists
Fixed usage of NGSI-LD Null in Attributes' definitions
December 2022
1.6.4
From 188r2_Registration_Clarifications
December 2022
1.6.4
From 182r2_Add_NGSI-LD_Roles_for_Context_Registration
December 2022
1.6.4
From 156r2 VocabProperty/URI type coercion
December 2022
1.6.4
177r2 Clarify usage of Accept, Content-Type and Linked @context when forwarding to
partial Context Brokers
December 2022
1.6.4
178 Add Batch Query to Federation Ops
December 2022
1.6.4
183r1 Clarify Temporal query behaviour
December 2022
1.6.4
149r4 Forbid scoped and nested @contexts
December 2022
1.6.4
023006 Fixing CSource registration example in C.3
December 2022
1.6.4
Fix: Tenants URI becomes String
December 2022
1.6.4
Fix: POST-QUERY-COUNT-PAGINATION
December 2022
1.6.4
Fix: CHECK-URI-PARAM
December 2022
1.6.4
Updated examples and text to context v1.7.jsonld
March 2023
1.6.6
CIM(23)000006_Adding_previousValue_to_GeoProperty_type_definition
March 2023
1.6.6
cSource -> CSource; "implementations shall do the following"
March 2023
1.6.7
000013r4_Updated_Distributed_Execution_Behaviour
March 2023
1.6.8
CIM(22)000195r3_type_passing_for_M2M_callReviewed
April 2023
1.6.9
Fixing URIString datatypes
June 2023
1.7.2
CIM(23)000053r1_Entity_Graph_Retrieval (for FIWARE SUMMIT)
June 2023
1.7.3
000056r2_APIv172_towards_v18.docx (for FIWARE SUMMIT)
October 2023
1.7.4
From 25023r2: Use Temporal Evolution instead of Temporal Representation +
Updated figures in clause 5 and 6
November 2023
1.7.5
From 68r5: Additional id only format and attribute projection via pick and omit
From 121r1: Relationship as Array
From 123r1: URN Namespace Definitions
From 149r3: Allow Broader Local Requests
From 153r1: JsonProperty
From 159: Bug fixed in CIM 009: GeoJSON responses in figures
From 160: Replace Attribute fix
December 2023
1.7.6
From 164: Host alias /info Endpoint
From 154r2: EntityMap
Updated figure in clause 6.2
January 2024
1.7.7
From 168r1: 504 error instead of 503 in JSON-LD context endpoints
From 169r1: Allow forbidden characters
From 170: Remove Scope from PATCH /attrs operations
From 1005r2: URI for value of several attributes
From 173r2: Clarify match in distributed operations
From 174: Protect core context
From 25002r2: API Issue #10 Filter on value with specific datasetId
January 2024
1.7.8
From 164r5: fix Tenant in Host Alias (164) and /info/sourceIdentity Endpoint + figure
Updated figure in clause 6.2
Updated figures in clause 4.2
January 2024
1.7.9
CIM(24)000007r2_Query_Language_Extension_for_Linked_Entity_Retrieval
January 2024
1.7.10
Internal revision, cleanup
January 2024
1.7.10
FIX: CIM(24)000015_Projection_attributes_error_messaging
January 2024
1.7.10
FIX: CIM(24)000014r1_POST_Query_Parameters
January 2024
1.7.11
TO revision
January 2024
1.7.12
ISG CIM revision and cleanup after TO revision. New keywords
February 2024
1.7.13
Footnotes in Tables
February 2024
1.7.15
Change of NGSILDTerm style to HTML Keyboard
February 2024
1.7.16
Added expiresAt to @context serving
March 2024
1.8.1
Published
April 2024
1.8.2
Clone of published
April 2024
1.8.3
Minor typos and style cleanup
April 2024
1.8.3
000048_Fix_operations_allowing_sysAttrs.docx
April 2024
1.8.3
000047r2_Accept_header_in_case_of_partial_success__207_.docx
April 2024
1.8.3
000049r1_Clarify__options__allowed_for_POST_queries.docx
April 2024
1.8.4
Track changes removed
May 2024
1.8.5
TooLargeResponse
June 2024
1.8.6
CIM(24)000036r1_Loop_Detection


Date
Version
Information about changes
June 2024
1.8.7
CIM(24)000033r6_Backwards_Compatibility_Indicators
June 2024
1.8.8
CIM(24)000027r5_Value_Type
June 2024
1.8.9
CIM(24)000029_Purge_Entities
June 2024
1.8.10
CIM(24)000028r4_Transient_Entity_bugfixed
June 2024
1.8.11
CIM(24)000070_Additional_Updates_to_ExpiresAt__Conformance_etc_
October 2024
1.8.12
CIM(24)000076r8_Entity_Maps_and_Split_Entities
October 2024
1.8.13
CIM(24)000108_Adding_Missing_Elements_in_Core_Context_and_Data_Types.zip
October 2024
1.8.14
Updated figures and new baseline and created Stable Draft out of this
November 2024
1.8.15
CIM(24)000128r1_Signed_Attributes_and_Scoped_Context
December 2024
1.8.16
VocabProperty instead of VocabularyProperty in C2.2 and Table 5.2.35-1
January 2025
1.8.17
Reordering table rows in alpha (almost) order
January 2025
1.8.18
Ngsildproof example and @context. Switching to new @context URI for v1.9
February 2025
1.8.19
CIM(25)000005 temporal bounds _clarifications_and_misc_fixes
March 2025
1.8.20
CIM(24)000111r5_Ordered_Entites
March 2025
1.8.21
CIM(24)000138r4_Snapshot
March 2025
1.8.22
CLEAN. Removed all track changes. Comments to be still addressed
March 2025
1.8.23
CIM(25)000011r1 Updated figures and text for Snapshots (clauses 5 and 6)
March 2025
1.8.24
CIM(25)000012r1 Harmonize output and 203
April 2025
1.8.25
CIM(25)000014 GS_CIM_009_v1825_fixSubscriptions
April 2025
1.8.26
CIM(25)000016 Explanation of valueType as data type
April 2025
1.8.27
CIM(25)000015 Precision Clarification + Harmonize captions in clause 6 + addressing
fixes requested in the comments
May 2025
1.8.28
CIM(25)000023 Order Table rows + small editorial fixes + aggregation parameters for
POST query
May 2025
1.8.29
Final review prior to EditHelp
June 2025
1.8.30
TO review: fuzzy figures, table numbering. Editorial: fixed core context and value in
ngsildproof example
