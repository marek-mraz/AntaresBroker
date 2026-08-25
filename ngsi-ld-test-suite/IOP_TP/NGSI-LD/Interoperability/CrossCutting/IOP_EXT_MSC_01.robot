*** Settings ***
Documentation       Cross-cutting distributed behaviours (Antares extension
...                 IOP TPs). 5.7.2.4: "If in the process of obtaining the
...                 query result it is necessary to issue a Context Source
...                 discovery operation, the same Context Source filter
...                 input parameter (if present) shall be propagated" — the
...                 csf gates which Context Sources are considered.
...                 4.5.16/5.7.1.4 GeoJSON retrieve; 4.15/4.5.18 language
...                 filter. 5.16.1.4: snapshot queries follow "the
...                 behaviour described in clause 5.7.2.4" (i.e. the
...                 distributed path); snapshotStatus is "success, if all
...                 queries ... were executed successfully and yielded at
...                 least one result, partial, if at least one query ...
...                 yielded a result". 6.3.13 count over the deduplicated
...                 4.3.6.4 union. 4.5.2/4.8: system generated timestamps
...                 of the remote data survive aggregation.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary
Library             HttpCtrl.Server

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}
${mock_host}        127.0.0.1
${mock_port}        8089
${dead_endpoint}    http://127.0.0.1:8099


*** Test Cases ***
IOP_EXT_MSC_01_01 csf Consults Only Matching Sources — The Excluded Mock Sees Nothing
    [Documentation]    5.7.2.4 csf: only Context Sources whose Context
    ...    Source Properties match the filter are considered — the
    ...    non-matching mock's hit count stays zero.
    [Tags]    iop    iop-ext    5_7_2    4_9    since_v1.9.1
    Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    []
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${sensor}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint, "sourceType": {"type": "Property", "value": "sensor"}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${sensor}
    Check Response Status Code    201    ${created.status_code}
    ${sim}=    Evaluate
    ...    {"id": $registration_id + "-2", "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port), "sourceType": {"type": "Property", "value": "simulator"}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${sim}
    Check Response Status Code    201    ${created.status_code}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    csf=sourceType=="sensor"
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-r
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    0

IOP_EXT_MSC_01_02 csf Comparisons Over Context Source Properties Gate Forwards
    [Documentation]    5.7.2.4 csf per 4.9 ("to filter out Context Sources
    ...    by the values of properties that describe them") — a numeric
    ...    comparison keeps only the heavier-weighted registration's
    ...    source. (Reworded from sysAttrs-timerel: the csf targets Context
    ...    Source Properties; querying registration system timestamps is
    ...    not grounded in the clause text.)
    [Tags]    iop    iop-ext    5_7_2    4_9    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${e2}=    Broker Base Of    ${b2_url}
    ${e3}=    Broker Base Of    ${b3_url}
    ${light}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $e2, "sourceWeight": {"type": "Property", "value": 1}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${light}
    Check Response Status Code    201    ${created.status_code}
    ${heavy}=    Evaluate
    ...    {"id": $registration_id + "-2", "type": "ContextSourceRegistration", "information": $info, "endpoint": $e3, "sourceWeight": {"type": "Property", "value": 5}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${heavy}
    Check Response Status Code    201    ${created.status_code}
    ${old}=    Simple Vehicle Entity    ${entity_id}-old    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${old}
    ${young}=    Simple Vehicle Entity    ${entity_id}-new    ${etype}    2
    Create Entity At Broker    ${b3_url}    ${young}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    csf=sourceWeight>3
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-new
    Should Not Contain    ${response.text}    ${entity_id}-old

IOP_EXT_MSC_01_03 GeoJSON Retrieve Of A Split Entity Uses The Remote Geometry
    [Documentation]    4.5.16/5.7.1.4: the GeoJSON Feature takes its
    ...    geometry from the default location GeoProperty — held remotely —
    ...    while the merged properties carry the local attribute.
    [Tags]    iop    iop-ext    4_5_16    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}}}
    Create Entity At Broker    ${b2_url}    ${remote}

    &{headers}=    Create Dictionary    Accept=application/geo+json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    GET    url=${b1_url}/entities/${eid}    headers=${headers}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${doc}=    Evaluate    $response.json()
    Should Be Equal    ${doc['type']}    Feature
    Should Be Equal    ${doc['geometry']['type']}    Point
    Should Contain    ${response.text}    Mercedes
    Should Not Be Equal    ${doc['geometry']}    ${None}

IOP_EXT_MSC_01_04 lang=* Converts The Remote LanguageProperty To One Language
    [Documentation]    4.15: lang="*" means "return LanguageProperties as a
    ...    string in any supported language", and "the attribute in
    ...    question shall be augmented with an additional non-reified
    ...    subproperty lang indicating the actual language returned" —
    ...    through the federation merge. (Reworded: the full-languageMap
    ...    expectation contradicted the 4.15 wildcard example.)
    [Tags]    iop    iop-ext    4_15    4_5_18    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "name": {"type": "LanguageProperty", "languageMap": {"en": "car", "de": "auto"}}}
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    lang=*
    Check Response Status Code    200    ${response.status_code}
    ${name}=    Evaluate    $response.json()['name']
    Should Be Equal    ${name['type']}    Property
    Dictionary Should Contain Key    ${name}    lang
    Should Be True    $name['value'] in ("car", "auto")
    Should Not Contain    ${response.text}    languageMap

IOP_EXT_MSC_01_05 A Snapshot Fill Walks The Distributed Path
    [Documentation]    5.16.1.4: "Implementations shall execute the Queries
    ...    specified in the snapshotQueries member, in each case following
    ...    the behaviour described in clause 5.7.2.4" — the snapshot
    ...    contains the REMOTE entity and reaches snapshotStatus success.
    [Tags]    iop    iop-ext    5_16_1    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${loc}=    Create Snapshot At B1
    ...    {"type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": $etype}]}]}
    ${snapshot}=    Wait Until Snapshot Ready    ${loc}
    Should Be Equal    ${snapshot['snapshotStatus']}    success
    ${sid}=    Set Variable    ${snapshot['id']}
    &{sheaders}=    Create Dictionary    NGSILD-Snapshot=${sid}
    ${response}=    GET    url=${b1_url}/entities    params=type=${etype}    headers=${sheaders}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-r
    Should Not Contain    ${response.text}    ${entity_id}-x
    DELETE    url=${b1_url}${loc}    expected_status=any

IOP_EXT_MSC_01_06 An Empty-Yield Query Makes The Snapshot partial
    [Documentation]    5.16.1.4: snapshotStatus is "partial, if at least
    ...    one query or temporal query was executed successfully and
    ...    yielded a result" (but not all did) — one populated query plus
    ...    one whose only type has no entities anywhere. (Reworded from
    ...    per-source: 5.16.1.4 grades per QUERY, not per Context Source.)
    [Tags]    iop    iop-ext    5_16_1    5_2_42    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${loc}=    Create Snapshot At B1
    ...    {"type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": $etype}]}, {"type": "Query", "entities": [{"type": "Void" + $etype}]}]}
    ${snapshot}=    Wait Until Snapshot Ready    ${loc}
    Should Be Equal    ${snapshot['snapshotStatus']}    partial
    ${second}=    Evaluate    $snapshot['snapshotQueriesDetails'][1]['resultStatus']
    Should Not Be Equal    ${second}    success
    DELETE    url=${b1_url}${loc}    expected_status=any

IOP_EXT_MSC_01_07 count Over Three Brokers Equals The Deduplicated Union
    [Documentation]    6.3.13 + 4.3.6.4: the NGSILD-Results-Count reflects
    ...    the union after duplicate exclusion — a split entity across
    ...    B1+B2 plus a whole entity on B3 count as 2, never 3.
    [Tags]    iop    iop-ext    6_3_13    4_3_6_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ${p1}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${p1}
    ${p2}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${p2}
    ${whole}=    Simple Vehicle Entity    ${entity_id}-w    ${etype}    2
    Create Entity At Broker    ${b3_url}    ${whole}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    count=true
    ...    splitEntities=true
    Check Response Status Code    200    ${response.status_code}
    ${count}=    Get From Dictionary    ${response.headers}    NGSILD-Results-Count
    Should Be Equal As Integers    ${count}    2
    Length Should Be    ${response.json()}    2

IOP_EXT_MSC_01_08 Remote System Timestamps Survive Aggregation
    [Documentation]    4.5.2/4.8 with 5.7.2.4: sysAttrs of the remote data
    ...    are the REMOTE system's timestamps — the forwarding broker must
    ...    not replace them with its own aggregation-time values.
    [Tags]    iop    iop-ext    4_5_2    4_8    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}
    ${direct}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-r    options=sysAttrs
    ${expected}=    Evaluate    $direct.json()['createdAt']
    Sleep    1.1s

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    options=sysAttrs
    Check Response Status Code    200    ${response.status_code}
    ${got}=    Evaluate
    ...    {e['id']: e.get('createdAt') for e in $response.json()}[$entity_id + "-r"]
    Should Be Equal    ${got}    ${expected}


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopMsc${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopMsc:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopmsc-${suffix}
    Set Test Variable    ${server_started}    ${False}

Start Mock
    Start Server    ${mock_host}    ${mock_port}
    Set Test Variable    ${server_started}    ${True}

Create Snapshot At B1
    [Arguments]    ${payload_expr}
    ${payload}=    Evaluate    ${payload_expr}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${b1_url}/snapshots    json=${payload}    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    ${loc}=    Get From Dictionary    ${response.headers}    Location
    ${loc}=    Evaluate    "${loc}".replace("/ngsi-ld/v1", "")
    RETURN    ${loc}

Wait Until Snapshot Ready
    [Arguments]    ${loc}
    FOR    ${i}    IN RANGE    50
        ${response}=    GET    url=${b1_url}${loc}    expected_status=any
        Check Response Status Code    200    ${response.status_code}
        IF    "${response.json()}[snapshotStatus]" != "preparing"    BREAK
        Sleep    0.2s
    END
    RETURN    ${response.json()}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-2
    FOR    ${tail}    IN    ${EMPTY}    -r    -w    -old    -new
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b3_url}    ${entity_id}${tail}
    END
    IF    ${server_started}
        Stop Server
    END
