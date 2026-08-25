*** Settings ***
Documentation       Two-broker federated-query edges (Antares extension IOP
...                 TPs). Data is distributed across B1 and B2; every query
...                 goes through B1. Covers the 5.7.2.4 union-and-merge, the
...                 q / geoquery / idPattern filters over federated results,
...                 count over the union (6.3.10) and graceful degradation
...                 with an unreachable peer (6.3.17).

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}


*** Test Cases ***
IOP_EXT_QRY_01_01 Query Returns The Union Without Duplicates
    [Documentation]    5.7.2.4: E1 lives in B1, E2 in B2, and E1 ALSO has a
    ...    fragment in B2 — the union via B1 is exactly two entities, E1
    ...    merged (4.5.5), never duplicated.
    [Tags]    iop    iop-ext    5_7_2    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e1}=    Simple Vehicle Entity    ${entity_id}-1    ${etype}    10
    Create Entity At Broker    ${b1_url}    ${e1}
    ${e1_fragment}=    Evaluate
    ...    {"id": $entity_id + "-1", "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${e1_fragment}
    ${e2}=    Simple Vehicle Entity    ${entity_id}-2    ${etype}    20
    Create Entity At Broker    ${b2_url}    ${e2}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    ${count_e1}=    Evaluate    $response.text.count($entity_id + "-1")
    Should Be Equal As Integers    ${count_e1}    1
    ${merged}=    Evaluate    {e["id"]: e for e in $response.json()}[$entity_id + "-1"]
    Dictionary Should Contain Key    ${merged}    brandName
    Dictionary Should Contain Key    ${merged}    speed

IOP_EXT_QRY_01_02 Filter Satisfied Only By Remote Data
    [Documentation]    5.7.2.4 + 4.9: q=speed>50 matches only the entity
    ...    held in B2 — it must be returned via B1; B1's own entity (speed
    ...    5) must not.
    [Tags]    iop    iop-ext    5_7_2    4_9    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${slow}=    Simple Vehicle Entity    ${entity_id}-slow    ${etype}    5
    Create Entity At Broker    ${b1_url}    ${slow}
    ${fast}=    Simple Vehicle Entity    ${entity_id}-fast    ${etype}    99
    Create Entity At Broker    ${b2_url}    ${fast}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    q=speed>50
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Contain    ${response.text}    ${entity_id}-fast
    Should Not Contain    ${response.text}    ${entity_id}-slow

IOP_EXT_QRY_01_03 Geoquery Matches A Remote Location
    [Documentation]    5.7.2.4 + 4.10: the near-point geoquery is satisfied
    ...    only by the entity in B2; B1's far-away entity is excluded.
    [Tags]    iop    iop-ext    5_7_2    4_10    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${near}=    Evaluate
    ...    {"id": $entity_id + "-near", "type": $etype, "speed": {"type": "Property", "value": 1}, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [8.6, 41.2]}}}
    Create Entity At Broker    ${b2_url}    ${near}
    ${far}=    Evaluate
    ...    {"id": $entity_id + "-far", "type": $etype, "speed": {"type": "Property", "value": 1}, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [17.1, 48.1]}}}
    Create Entity At Broker    ${b1_url}    ${far}

    ${response}=    Query Entities Via Broker    ${b1_url}
    ...    type=${etype}
    ...    georel=near;maxDistance==1000
    ...    geometry=Point
    ...    coordinates=[8.6,41.2]
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Contain    ${response.text}    ${entity_id}-near
    Should Not Contain    ${response.text}    ${entity_id}-far

IOP_EXT_QRY_01_04 Count Includes Remote Matches
    [Documentation]    6.3.10 over 5.7.2.4: one local + two remote matching
    ...    entities → NGSILD-Results-Count is 3.
    [Tags]    iop    iop-ext    5_7_2    6_3_10    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Simple Vehicle Entity    ${entity_id}-a    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${local}
    ${r1}=    Simple Vehicle Entity    ${entity_id}-b    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r1}
    ${r2}=    Simple Vehicle Entity    ${entity_id}-c    ${etype}    3
    Create Entity At Broker    ${b2_url}    ${r2}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    count=true
    Check Response Status Code    200    ${response.status_code}
    ${count}=    Get From Dictionary    ${response.headers}    NGSILD-Results-Count
    Should Be Equal As Integers    ${count}    3

IOP_EXT_QRY_01_05 IdPattern Filters The Federated Union
    [Documentation]    5.7.2.4 + 5.2.33: idPattern applies to remote
    ...    candidates too — only the B2 entity whose id matches the pattern
    ...    is returned.
    [Tags]    iop    iop-ext    5_7_2    5_2_33    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${match}=    Simple Vehicle Entity    ${entity_id}-wanted    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${match}
    ${nomatch}=    Simple Vehicle Entity    ${entity_id}-other    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${nomatch}

    ${response}=    Query Entities Via Broker    ${b1_url}
    ...    type=${etype}
    ...    idPattern=.*wanted$
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Contain    ${response.text}    ${entity_id}-wanted
    Should Not Contain    ${response.text}    ${entity_id}-other

IOP_EXT_QRY_01_06 An Unreachable Peer Degrades Gracefully
    [Documentation]    6.3.17: a registration pointing at a dead endpoint
    ...    must not fail the query — B1 answers 200 with its local results
    ...    and signals the failed part via NGSILD-Warning.
    [Tags]    iop    iop-ext    5_7_2    6_3_17    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    http://127.0.0.1:59999/ngsi-ld/v1    ${etype}
    ${local}=    Simple Vehicle Entity    ${entity_id}-local    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${local}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-local
    Dictionary Should Contain Key    ${response.headers}    NGSILD-Warning


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}

Cleanup Interop Fixtures
    FOR    ${tail}    IN    -1    -2    -slow    -fast    -near    -far    -a    -b    -c    -wanted    -other    -local
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
    END
    Delete Registration At Broker    ${b1_url}    ${registration_id}
