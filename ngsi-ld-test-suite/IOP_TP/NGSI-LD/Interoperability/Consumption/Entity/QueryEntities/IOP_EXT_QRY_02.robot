*** Settings ***
Documentation       Federated-query variants ported from the single-broker
...                 TP tree to real multi-broker setups (Antares extension
...                 IOP TPs): representations over the union, the 5.7.2.4
...                 split-entities aggregate filter, scopeQ, csf against
...                 real registrations, three-broker unions, the
...                 orderBy-distributed 400, local=true scoping and
...                 pagination links over the federated union.

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
IOP_EXT_QRY_02_01 KeyValues Over The Federated Union
    [Documentation]    4.5.3 applied by B1 to a union spanning both brokers.
    [Tags]    iop    iop-ext    4_5_3    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    options=keyValues
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    ${values}=    Evaluate    sorted(e["speed"] for e in $response.json())
    Should Be Equal    ${values}    ${{ [1, 2] }}

IOP_EXT_QRY_02_02 Attrs Projection Over The Federated Union
    [Documentation]    5.7.2.4 attrs: the projection applies to remote
    ...    entities too — brandName never appears, entities without speed
    ...    drop out.
    [Tags]    iop    iop-ext    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${with_speed}=    Evaluate
    ...    {"id": $entity_id + "-s", "type": $etype, "speed": {"type": "Property", "value": 1}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${with_speed}
    ${without}=    Evaluate
    ...    {"id": $entity_id + "-n", "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}
    Create Entity At Broker    ${b2_url}    ${without}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    attrs=speed
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Contain    ${response.text}    ${entity_id}-s
    Should Not Contain    ${response.text}    brandName

IOP_EXT_QRY_02_03 Split-Entities Flag Turns The Aggregate Filter On
    [Documentation]    5.7.2.4: q=speed>20;brandName=="Mercedes" is
    ...    satisfiable only by the AGGREGATED entity (speed in B2, brandName
    ...    in B1). With splitEntities=true the filters are stripped from the
    ...    forwards and applied after aggregation — the entity matches.
    [Tags]    iop    iop-ext    5_7_2    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local_half}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local_half}
    ${remote_half}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${remote_half}

    ${response}=    Query Entities Via Broker    ${b1_url}
    ...    type=${etype}
    ...    splitEntities=true
    ...    q=speed>20;brandName=="Mercedes"
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}
    ${merged}=    Evaluate    {e["id"]: e for e in $response.json()}[$entity_id]
    Dictionary Should Contain Key    ${merged}    speed
    Dictionary Should Contain Key    ${merged}    brandName

IOP_EXT_QRY_02_04 ScopeQ Matches A Remote Scope
    [Documentation]    4.19/5.7.2.4: scopeQ=/Madrid selects only the B2
    ...    entity carrying that scope.
    [Tags]    iop    iop-ext    4_19    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${scoped}=    Evaluate
    ...    {"id": $entity_id + "-in", "type": $etype, "scope": "/Madrid", "speed": {"type": "Property", "value": 1}}
    Create Entity At Broker    ${b2_url}    ${scoped}
    ${unscoped}=    Simple Vehicle Entity    ${entity_id}-out    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${unscoped}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    scopeQ=/Madrid
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-in
    Should Not Contain    ${response.text}    ${entity_id}-out

IOP_EXT_QRY_02_05 Union Across Three Brokers
    [Documentation]    4.3.6.2: two inclusive registrations (B2 and B3) —
    ...    the query via B1 unions all three brokers' entities.
    [Tags]    iop    iop-ext    4_3_6_2    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}
    ${e1}=    Simple Vehicle Entity    ${entity_id}-b1    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e1}
    ${e2}=    Simple Vehicle Entity    ${entity_id}-b2    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${e2}
    ${e3}=    Simple Vehicle Entity    ${entity_id}-b3    ${etype}    3
    Create Entity At Broker    ${b3_url}    ${e3}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    3

IOP_EXT_QRY_02_06 Csf Gates Real Context Sources
    [Documentation]    5.7.2.4 csf: B2's registration carries the Context
    ...    Source Property sourceType="sensor", B3's does not — the csf
    ...    keeps only B2's source, so only B2's entity is returned.
    [Tags]    iop    iop-ext    5_7_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "inclusive", "information": $info, "endpoint": __import__('re').sub("/ngsi-ld/v1$", "", $b2_url), "sourceType": {"type": "Property", "value": "sensor"}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${b1_url}/csourceRegistrations    json=${reg}    headers=${headers}    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}
    ${e2}=    Simple Vehicle Entity    ${entity_id}-b2    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${e2}
    ${e3}=    Simple Vehicle Entity    ${entity_id}-b3    ${etype}    3
    Create Entity At Broker    ${b3_url}    ${e3}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    csf=sourceType=="sensor"
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-b2
    Should Not Contain    ${response.text}    ${entity_id}-b3

IOP_EXT_QRY_02_07 OrderBy Is Rejected On A Distributed Query
    [Documentation]    5.7.2.4: "If the ordering parameter is present and
    ...    the execution of the operation is not limited to the local scope
    ...    … BadRequestData" — with a live registration, orderBy is a 400.
    [Tags]    iop    iop-ext    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e}=    Simple Vehicle Entity    ${entity_id}-b1    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    orderBy=speed
    Check Response Status Code    400    ${response.status_code}

IOP_EXT_QRY_02_08 local=true Excludes Remote Data
    [Documentation]    5.5.13/6.3.18: the local scope stops federation —
    ...    only B1's own entity is returned and no NGSILD-Warning appears.
    [Tags]    iop    iop-ext    5_5_13    6_3_18    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${l}=    Simple Vehicle Entity    ${entity_id}-b1    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}
    ${r}=    Simple Vehicle Entity    ${entity_id}-b2    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-b1
    Should Not Contain    ${response.text}    ${entity_id}-b2

IOP_EXT_QRY_02_09 Pagination Links Walk The Federated Union
    [Documentation]    5.5.9/6.3.10 over 5.7.2.4: limit=2 over a 3-entity
    ...    union — first page is full with a next link, the second page
    ...    holds the remainder, and the two pages union to all three ids.
    [Tags]    iop    iop-ext    5_5_9    6_3_10    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e1}=    Simple Vehicle Entity    ${entity_id}-a    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e1}
    ${e2}=    Simple Vehicle Entity    ${entity_id}-b    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${e2}
    ${e3}=    Simple Vehicle Entity    ${entity_id}-c    ${etype}    3
    Create Entity At Broker    ${b2_url}    ${e3}

    ${page1}=    Query Entities Via Broker    ${b1_url}    type=${etype}    limit=2
    Check Response Status Code    200    ${page1.status_code}
    Length Should Be    ${page1.json()}    2
    ${link}=    Get From Dictionary    ${page1.headers}    Link
    Should Contain    ${link}    rel="next"
    ${page2}=    Query Entities Via Broker    ${b1_url}    type=${etype}    limit=2    offset=2
    Check Response Status Code    200    ${page2.status_code}
    Length Should Be    ${page2.json()}    1
    ${union}=    Evaluate    {e["id"] for e in $page1.json()} | {e["id"] for e in $page2.json()}
    Length Should Be    ${union}    3

IOP_EXT_QRY_02_10 Multi-Type Selector Spans Brokers
    [Documentation]    4.17/5.7.2.4: type=A,B — B1 holds an A, B2 holds a B
    ...    (registered for both types); both come back in one union.
    [Tags]    iop    iop-ext    4_17    5_7_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}, {"type": $etype + "B"}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${a}=    Simple Vehicle Entity    ${entity_id}-a    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${a}
    ${b}=    Evaluate
    ...    {"id": $entity_id + "-b", "type": $etype + "B", "speed": {"type": "Property", "value": 2}}
    Create Entity At Broker    ${b2_url}    ${b}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype},${etype}B
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    Should Contain    ${response.text}    ${entity_id}-a
    Should Contain    ${response.text}    ${entity_id}-b


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}

Cleanup Interop Fixtures
    FOR    ${tail}    IN    ${EMPTY}    -l    -r    -s    -n    -in    -out    -a    -b    -c    -b1    -b2    -b3
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b3_url}    ${entity_id}${tail}
    END
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-3
