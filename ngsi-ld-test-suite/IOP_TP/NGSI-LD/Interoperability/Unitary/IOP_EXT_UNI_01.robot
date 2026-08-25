*** Settings ***
Documentation       Unitary distributed query/retrieve (Antares extension
...                 IOP TPs). 4.3.6.7: "Entity data retrieval can be
...                 considered as a unitary operation, masking the fact that
...                 each registered Context Broker is receiving a separate
...                 distributed Context Consumption request". 5.7.2.4: with
...                 splitEntities "the filters ... shall be removed before
...                 forwarding the request. These filters then have to be
...                 applied after the Entity information from different
...                 Context Sources and local information, if there is any,
...                 has been aggregated"; remote Entity Arrays "are then
...                 merged together with the locally queried result
...                 according to the algorithm defined in clause 4.5.5".
...                 5.5.9.3: with Entity maps "the set of Entities
...                 considered for the result is fixed with the initial
...                 query creating the Entity map". 6.3.13: the total number
...                 of matching results is returned in NGSILD-Results-Count.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}


*** Test Cases ***
IOP_EXT_UNI_01_01 limit=1 Returns One Assembled Split Entity
    [Documentation]    4.3.6.7 unitary retrieval + 5.7.2.4 4.5.5-merge: an
    ...    entity whose halves live on B1 and B2 is one result — limit=1
    ...    yields exactly one element carrying both halves.
    [Tags]    iop    iop-ext    4_3_6_7    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    splitEntities=true    limit=1
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    ${merged}=    Evaluate    $response.json()[0]
    Dictionary Should Contain Key    ${merged}    speed
    Dictionary Should Contain Key    ${merged}    brandName

IOP_EXT_UNI_01_02 A Split Entity Counts Once In NGSILD-Results-Count
    [Documentation]    6.3.13 ("the total number of matching results ... is
    ...    returned") over the 5.7.2.4 union: a split entity is one result —
    ...    the count is 2 (split + whole), never 3.
    [Tags]    iop    iop-ext    6_3_13    5_7_2    4_3_6_7    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    Create Entity At Broker    ${b2_url}    ${remote}
    ${whole}=    Simple Vehicle Entity    ${entity_id}-f    ${etype}    6
    Create Entity At Broker    ${b2_url}    ${whole}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    splitEntities=true    count=true
    Check Response Status Code    200    ${response.status_code}
    ${count}=    Get From Dictionary    ${response.headers}    NGSILD-Results-Count
    Should Be Equal As Integers    ${count}    2
    Length Should Be    ${response.json()}    2

IOP_EXT_UNI_01_03 Three-Way Split: q Holds Only On The Full Aggregate
    [Documentation]    5.7.2.4: with splitEntities=true the query filters
    ...    "shall be removed before forwarding" and "applied after the
    ...    Entity information from different Context Sources and local
    ...    information ... has been aggregated" — a conjunctive q whose
    ...    terms live on B1, B2 and B3 matches only the aggregate.
    [Tags]    iop    iop-ext    5_7_2    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}
    ${p1}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${p1}
    ${p2}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${p2}
    ${p3}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "color": {"type": "Property", "value": "red"}}
    Create Entity At Broker    ${b3_url}    ${p3}
    ${decoy}=    Simple Vehicle Entity    ${entity_id}-x    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${decoy}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    splitEntities=true    q=speed>20;brandName=="Mercedes";color=="red"
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    ${merged}=    Evaluate    $response.json()[0]
    Should Be Equal    ${merged['id']}    ${entity_id}
    Dictionary Should Contain Key    ${merged}    speed
    Dictionary Should Contain Key    ${merged}    color
    Should Not Contain    ${response.text}    ${entity_id}-x

IOP_EXT_UNI_01_04 Geo And q Filters Both Apply Post-Aggregation
    [Documentation]    5.7.2.4 aggregated filters: "the geospatial
    ...    restrictions imposed by the geoquery are met" on the AGGREGATE —
    ...    geometry lives on B2, the q attribute on B1; both must hold.
    [Tags]    iop    iop-ext    5_7_2    4_10    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}}}
    Create Entity At Broker    ${b2_url}    ${remote}

    ${hit}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    splitEntities=true    q=speed>0
    ...    georel=near;maxDistance==5000    geometry=Point    coordinates=[13.4,52.52]
    Check Response Status Code    200    ${hit.status_code}
    Length Should Be    ${hit.json()}    1
    Should Contain    ${hit.text}    ${entity_id}
    ${miss}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    splitEntities=true    q=speed>0
    ...    georel=near;maxDistance==5000    geometry=Point    coordinates=[2.35,48.85]
    Check Response Status Code    200    ${miss.status_code}
    Should Not Contain    ${miss.text}    ${entity_id}

IOP_EXT_UNI_01_05 pick Projects The Aggregate: A Remote-Only Attribute Survives
    [Documentation]    5.7.2.5/4.21 over the 5.7.2.4 union: the restrictive
    ...    member list reduces the AGGREGATED entity — the picked attribute
    ...    exists only remotely and survives; the local-only attribute is
    ...    removed.
    [Tags]    iop    iop-ext    5_7_2    4_21    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    7
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    splitEntities=true    pick=speed
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Contain    ${response.text}    speed
    Should Not Contain    ${response.text}    brandName

IOP_EXT_UNI_01_06 attrs Selector Matches Via A Remote-Only Attribute
    [Documentation]    5.7.2.4 aggregated filters: "if the Attribute list is
    ...    present, in order for an Entity to match, it shall contain at
    ...    least one of the Attributes in the projection Attribute list" —
    ...    checked on the aggregate, so a locally attribute-less entity
    ...    qualifies through its remote half; an entity without the
    ...    attribute anywhere does not.
    [Tags]    iop    iop-ext    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    7
    Create Entity At Broker    ${b2_url}    ${remote}
    ${nospeed}=    Evaluate
    ...    {"id": $entity_id + "-x", "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}
    Create Entity At Broker    ${b1_url}    ${nospeed}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    splitEntities=true    attrs=speed
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Contain    ${response.text}    ${entity_id}
    Should Not Contain    ${response.text}    ${entity_id}-x
    Should Not Contain    ${response.text}    brandName

IOP_EXT_UNI_01_07 A Pinned EntityMap Never Admits Later Arrivals
    [Documentation]    5.5.9.3: "the set of Entities considered for the
    ...    result is fixed with the initial query creating the Entity map"
    ...    + 4.3.6.7: "Only the Entities whose identifiers are contained in
    ...    the Entity map are considered when rendering the result pages" —
    ...    an entity created remotely mid-walk stays out of later pages.
    [Tags]    iop    iop-ext    4_3_6_7    5_5_9_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${a}=    Simple Vehicle Entity    ${entity_id}-a    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${a}
    ${b}=    Simple Vehicle Entity    ${entity_id}-b    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${b}

    ${created}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${created.status_code}
    ${map_ref}=    Get From Dictionary    ${created.headers}    NGSILD-EntityMap

    ${mid}=    Simple Vehicle Entity    ${entity_id}-late    ${etype}    3
    Create Entity At Broker    ${b2_url}    ${mid}

    ${page1}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}    limit=1
    Check Response Status Code    200    ${page1.status_code}
    ${page2}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}    limit=1    offset=1
    Check Response Status Code    200    ${page2.status_code}
    ${page3}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}    limit=1    offset=2
    Check Response Status Code    200    ${page3.status_code}
    ${union}=    Evaluate
    ...    {e["id"] for e in $page1.json()} | {e["id"] for e in $page2.json()} | {e["id"] for e in $page3.json()}
    Should Not Contain    ${union}    ${entity_id}-late
    Length Should Be    ${union}    2

IOP_EXT_UNI_01_08 Retrieve Merge: Newest observedAt Wins Per datasetId
    [Documentation]    4.3.6.3 + 4.5.5.3 over 5.7.1.4: "If two registered
    ...    Context Sources are providing context data for the same
    ...    Attribute, the Attribute instances can be distinguished by
    ...    datasetId. The mechanism for determining which data shall be
    ...    returned is defined in clause 4.5.5" — per datasetId the most
    ...    recent instance wins, regardless of which broker holds it.
    [Tags]    iop    iop-ext    4_3_6_3    4_5_5    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 101, "datasetId": "urn:ngsi-ld:ds:a", "observedAt": "2026-01-01T00:00:00Z"}, {"type": "Property", "value": 202, "datasetId": "urn:ngsi-ld:ds:b", "observedAt": "2026-06-01T00:00:00Z"}]}
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 909, "datasetId": "urn:ngsi-ld:ds:a", "observedAt": "2026-06-01T00:00:00Z"}, {"type": "Property", "value": 808, "datasetId": "urn:ngsi-ld:ds:b", "observedAt": "2026-01-01T00:00:00Z"}]}
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    909
    Should Contain    ${response.text}    202
    Should Not Contain    ${response.text}    101
    Should Not Contain    ${response.text}    808


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopUni${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopUni:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopuni-${suffix}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-3
    FOR    ${tail}    IN    ${EMPTY}    -f    -x    -a    -b    -late
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b3_url}    ${entity_id}${tail}
    END
