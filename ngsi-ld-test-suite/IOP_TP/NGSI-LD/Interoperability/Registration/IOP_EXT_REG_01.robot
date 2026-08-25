*** Settings ***
Documentation       Registration semantics & modes (Antares extension IOP
...                 TPs). 4.3.6.2: "This [inclusive] is the default mode of
...                 operation"; auxiliary "never overrides data held directly
...                 within a Context Broker". 4.3.6.3: proxied registrations
...                 — exclusive/redirect conflicts, overlapping redirects
...                 "operations are distributed to all registered Context
...                 Sources". 5.9.2.4/5.9.3.4: registration-vs-entity
...                 Conflict errors. 4.3.6.1: "Ultimately, all constraints
...                 specified in the registration shall be respected" —
...                 attribute/id/geo-scoped narrowing. 4.20: operation names,
...                 groups, default federationOps.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}


*** Test Cases ***
IOP_EXT_REG_01_01 Registration Without Mode Defaults To Inclusive
    [Documentation]    5.2.9 Table 5.2.9-1: "The mode is assumed to be
    ...    \"inclusive\" if not explicitly defined". 4.3.6.2: inclusive
    ...    distributes operations "even if relevant context data is
    ...    available directly within the Context Broker itself (in which
    ...    case, all results will be integrated in the final response)".
    [Tags]    iop    iop-ext    5_2_9    4_3_6_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    Should Contain    ${response.text}    ${entity_id}-l
    Should Contain    ${response.text}    ${entity_id}-r
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning

IOP_EXT_REG_01_02 Exclusive Registration Over Existing Local Attributes Is A Conflict
    [Documentation]    5.9.2.4: "If an Entity already exists for the supplied
    ...    Entity ID (URI) and the existing Entity contains any of the
    ...    Attributes defined in the registration, an error of type Conflict
    ...    shall be raised."
    [Tags]    iop    iop-ext    5_9_2    4_3_6_3    since_v1.9.1
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e}
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "exclusive", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    409    ${response.status_code}

    ${readback}=    Get Registration At Broker    ${b1_url}    ${registration_id}
    Check Response Status Code    404    ${readback.status_code}

IOP_EXT_REG_01_03 Redirect Registration Matching An Existing Entity Is A Conflict
    [Documentation]    5.9.2.4: "If the Context Source to be registered has
    ...    its mode property defined as redirect ... If an existing Entity
    ...    already matches the Context Source Registration, an error of type
    ...    Conflict shall be raised."
    [Tags]    iop    iop-ext    5_9_2    4_3_6_3    since_v1.9.1
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e}
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    409    ${response.status_code}

    ${readback}=    Get Registration At Broker    ${b1_url}    ${registration_id}
    Check Response Status Code    404    ${readback.status_code}

IOP_EXT_REG_01_04 Updating A Registration To Exclusive Over Local Attributes Is A Conflict
    [Documentation]    5.9.3.4 (Update Context Source Registration): "If the
    ...    Context Source Registration to be updated has its mode property
    ...    defined as exclusive ... If an Entity already exists for the
    ...    supplied Entity ID (URI) and the existing Entity contains any of
    ...    the Attributes defined in the registration, an error of type
    ...    Conflict shall be raised."
    [Tags]    iop    iop-ext    5_9_3    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e}

    ${fragment}=    Evaluate    {"mode": "exclusive"}
    ${response}=    Patch Registration At Broker    ${b1_url}    ${registration_id}    ${fragment}
    Check Response Status Code    409    ${response.status_code}

    ${readback}=    Get Registration At Broker    ${b1_url}    ${registration_id}
    Check Response Status Code    200    ${readback.status_code}
    Should Not Contain    ${readback.text}    exclusive

IOP_EXT_REG_01_05 Overlapping Redirect Registrations Distribute To All Sources
    [Documentation]    4.3.6.3: "In the case that multiple overlapping
    ...    redirect registrations are defined, operations are distributed to
    ...    all registered Context Sources."
    [Tags]    iop    iop-ext    4_3_6_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ...    mode=redirect
    ${e2}=    Simple Vehicle Entity    ${entity_id}-b2    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${e2}
    ${e3}=    Simple Vehicle Entity    ${entity_id}-b3    ${etype}    3
    Create Entity At Broker    ${b3_url}    ${e3}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    Should Contain    ${response.text}    ${entity_id}-b2
    Should Contain    ${response.text}    ${entity_id}-b3
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning

IOP_EXT_REG_01_06 Auxiliary Data Fills Gaps But Never Overrides Local Data
    [Documentation]    4.3.6.2: "An auxiliary Context Source Registration
    ...    never overrides data held directly within a Context Broker.
    ...    Context data from auxiliary context sources is only included if
    ...    it is supplementary to the context data otherwise available to
    ...    the Context Broker."
    [Tags]    iop    iop-ext    4_3_6_2    since_v1.9.1
    ${ops}=    Evaluate    ["retrieveEntity", "queryEntity"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=auxiliary    operations=${ops}
    ${local}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${local}
    ${aux}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 99}, "brandName": {"type": "Property", "value": "Aux"}}
    Create Entity At Broker    ${b2_url}    ${aux}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    ${speed}=    Evaluate    $response.json()["speed"]["value"]
    Should Be Equal As Integers    ${speed}    1
    ${brand}=    Evaluate    $response.json()["brandName"]["value"]
    Should Be Equal    ${brand}    Aux
    # negative: the aux value must appear NOWHERE as a value — asserted as
    # the JSON member, not a bare substring (a run-unique hex id like
    # b79dc993 contains "99" and red-flagged the bare form, 2026-08-16)
    Should Not Contain    ${response.text}    "value":99

IOP_EXT_REG_01_07 Inclusive Beats Auxiliary For The Same Attribute
    [Documentation]    4.3.6.2: auxiliary data "is only included if it is
    ...    supplementary to the context data otherwise available to the
    ...    Context Broker" — data from an inclusive source counts as
    ...    otherwise available, so the auxiliary instance is not served.
    [Tags]    iop    iop-ext    4_3_6_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${ops}=    Evaluate    ["retrieveEntity", "queryEntity"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ...    mode=auxiliary    operations=${ops}
    ${inc}=    Simple Vehicle Entity    ${entity_id}    ${etype}    222
    Create Entity At Broker    ${b2_url}    ${inc}
    ${aux}=    Simple Vehicle Entity    ${entity_id}    ${etype}    333
    Create Entity At Broker    ${b3_url}    ${aux}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    ${speed}=    Evaluate    $response.json()["speed"]["value"]
    Should Be Equal As Integers    ${speed}    222
    Should Not Contain    ${response.text}    333

IOP_EXT_REG_01_08 PropertyNames Scope Gates The Forward
    [Documentation]    4.3.6.1: "if a registration states that only Entities
    ...    of a given type are offered, the distributed request does not
    ...    contain additional types ... Ultimately, all constraints
    ...    specified in the registration shall be respected." 5.2.10
    ...    propertyNames: "Describes the Properties that the CSource may be
    ...    able to provide." A query for an attribute outside the
    ...    registration is not forwarded.
    [Tags]    iop    iop-ext    4_3_6_1    5_2_10    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"type": $etype}], "propertyNames": ["speed"]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${r}=    Evaluate
    ...    {"id": $entity_id + "-r", "type": $etype, "speed": {"type": "Property", "value": 7}, "brandName": {"type": "Property", "value": "Skoda"}}
    Create Entity At Broker    ${b2_url}    ${r}

    ${outside}=    Query Entities Via Broker    ${b1_url}    type=${etype}    attrs=brandName
    Check Response Status Code    200    ${outside.status_code}
    Should Not Contain    ${outside.text}    ${entity_id}-r
    ${inside}=    Query Entities Via Broker    ${b1_url}    type=${etype}    attrs=speed
    Check Response Status Code    200    ${inside.status_code}
    Should Contain    ${inside.text}    ${entity_id}-r
    Should Contain    ${inside.text}    speed
    Should Not Contain    ${inside.text}    brandName

IOP_EXT_REG_01_09 RelationshipNames Scope Gates The Forward
    [Documentation]    5.2.10 relationshipNames: "Describes the
    ...    Relationships that the CSource may be able to provide." 4.3.6.1:
    ...    all constraints specified in the registration shall be respected
    ...    — a query for an attribute not named by the registration is not
    ...    forwarded, and the forward is narrowed to the registered names.
    [Tags]    iop    iop-ext    4_3_6_1    5_2_10    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"type": $etype}], "relationshipNames": ["knows"]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${r}=    Evaluate
    ...    {"id": $entity_id + "-r", "type": $etype, "knows": {"type": "Relationship", "object": "urn:ngsi-ld:Target:1"}, "speed": {"type": "Property", "value": 7}}
    Create Entity At Broker    ${b2_url}    ${r}

    ${inside}=    Query Entities Via Broker    ${b1_url}    type=${etype}    attrs=knows
    Check Response Status Code    200    ${inside.status_code}
    Should Contain    ${inside.text}    ${entity_id}-r
    Should Contain    ${inside.text}    knows
    Should Not Contain    ${inside.text}    speed
    ${outside}=    Query Entities Via Broker    ${b1_url}    type=${etype}    attrs=speed
    Check Response Status Code    200    ${outside.status_code}
    Should Not Contain    ${outside.text}    ${entity_id}-r

IOP_EXT_REG_01_10 IdPattern Scope: Non-Matching Retrieve Stays Local
    [Documentation]    5.2.8 EntityInfo idPattern via 5.2.9/5.2.10 + 4.3.6.1
    ...    (registration constraints shall be respected): a retrieve whose
    ...    id does not match the registered idPattern is served locally
    ...    without contacting the source; a matching id is forwarded.
    [Tags]    iop    iop-ext    4_3_6_1    5_2_8    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"idPattern": "urn:ngsi-ld:IopRegA:.*", "type": $etype}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${local}=    Simple Vehicle Entity    urn:ngsi-ld:IopRegB:${suffix}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${local}
    ${shadow}=    Evaluate
    ...    {"id": "urn:ngsi-ld:IopRegB:" + $suffix, "type": $etype, "remoteMarker": {"type": "Property", "value": "leak"}}
    Create Entity At Broker    ${b2_url}    ${shadow}
    ${match}=    Evaluate
    ...    {"id": "urn:ngsi-ld:IopRegA:" + $suffix, "type": $etype, "remoteMarker": {"type": "Property", "value": "fed"}}
    Create Entity At Broker    ${b2_url}    ${match}

    ${response}=    Get Entity Via Broker    ${b1_url}    urn:ngsi-ld:IopRegB:${suffix}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    speed
    Should Not Contain    ${response.text}    remoteMarker
    ${forwarded}=    Get Entity Via Broker    ${b1_url}    urn:ngsi-ld:IopRegA:${suffix}
    Check Response Status Code    200    ${forwarded.status_code}
    Should Contain    ${forwarded.text}    remoteMarker

IOP_EXT_REG_01_11 Operations Subset: retrieveEntity Only
    [Documentation]    4.20/4.3.6.1: "registered Context Sources may
    ...    indicate that they are only willing to respond to a limited
    ...    subset of API operations. Context Brokers shall respect this, to
    ...    avoid unnecessarily sending distributed operation requests which
    ...    are always guaranteed to fail." operations=["retrieveEntity"] —
    ...    queryEntity is not forwarded, retrieveEntity is.
    [Tags]    iop    iop-ext    4_20    4_3_6_1    since_v1.9.1
    ${ops}=    Evaluate    ["retrieveEntity"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${ops}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    7
    Create Entity At Broker    ${b2_url}    ${r}

    ${query}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${query.status_code}
    Should Not Contain    ${query.text}    ${entity_id}-r
    ${retrieve}=    Get Entity Via Broker    ${b1_url}    ${entity_id}-r
    Check Response Status Code    200    ${retrieve.status_code}
    Should Contain    ${retrieve.text}    speed

IOP_EXT_REG_01_12 Operation Group redirectionOps Expands To Its Members
    [Documentation]    4.20 Table 4.20-2: redirectionOps implements
    ...    createEntity, retrieveEntity, queryEntity (among others) — a
    ...    create matching a redirect registration whose operations name
    ...    only the group is forwarded, and the entity is held remotely
    ...    only (4.3.6.3: "The Context Broker itself holds no data locally
    ...    in conflict to the registration").
    [Tags]    iop    iop-ext    4_20    4_3_6_3    since_v1.9.1
    ${ops}=    Evaluate    ["redirectionOps"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${ops}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${b1_url}/entities    json=${e}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    200    ${at_b2.status_code}
    ${local_only}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${local_only.status_code}
    ${federated}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${federated.status_code}

IOP_EXT_REG_01_13 Default Operations Are federationOps: Create Is Not Forwarded
    [Documentation]    5.2.9/4.20: "If no specific subset of operations is
    ...    defined for a Context Source Registration, the default set of
    ...    operations matches the group defined as \"federationOps\"" —
    ...    whose member list has no createEntity. A create matching a
    ...    default-ops inclusive registration is served locally only.
    [Tags]    iop    iop-ext    4_20    5_6_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    Create Entity At Broker    ${b1_url}    ${e}

    ${at_b2}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${at_b2.status_code}
    ${local}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${local.status_code}

IOP_EXT_REG_01_14 Geo-Scoped Registration Gates The Forward
    [Documentation]    5.2.9 location: "Location for which the Context
    ...    Source may be able to provide information." + 4.3.6.1: all
    ...    constraints specified in the registration shall be respected — a
    ...    geo query outside the registered geometry is not forwarded, one
    ...    intersecting it is.
    [Tags]    iop    iop-ext    5_2_9    4_3_6_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint, "location": {"type": "Polygon", "coordinates": [[[13.0, 52.2], [13.8, 52.2], [13.8, 52.8], [13.0, 52.8], [13.0, 52.2]]]}}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}
    ${berlin}=    Evaluate
    ...    {"id": $entity_id + "-berlin", "type": $etype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}}}
    Create Entity At Broker    ${b2_url}    ${berlin}
    ${paris}=    Evaluate
    ...    {"id": $entity_id + "-paris", "type": $etype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.35, 48.85]}}}
    Create Entity At Broker    ${b2_url}    ${paris}

    ${near_paris}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    georel=near;maxDistance==5000    geometry=Point    coordinates=[2.35,48.85]
    Check Response Status Code    200    ${near_paris.status_code}
    Should Not Contain    ${near_paris.text}    ${entity_id}-paris
    ${near_berlin}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    georel=near;maxDistance==5000    geometry=Point    coordinates=[13.4,52.52]
    Check Response Status Code    200    ${near_berlin.status_code}
    Should Contain    ${near_berlin.text}    ${entity_id}-berlin
    Should Not Contain    ${near_berlin.text}    ${entity_id}-paris


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopReg${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopReg:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopreg-${suffix}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-2
    FOR    ${eid}    IN    ${entity_id}    ${entity_id}-l    ${entity_id}-r    ${entity_id}-b2
    ...    ${entity_id}-b3    ${entity_id}-berlin    ${entity_id}-paris
    ...    urn:ngsi-ld:IopRegA:${suffix}    urn:ngsi-ld:IopRegB:${suffix}
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
        Delete Entity Via Broker    ${b3_url}    ${eid}
    END
