*** Settings ***
Documentation       Query-entities routing: query id/idPattern crossed with
...                 CSR EntityInfo id/idPattern (Antares extension IOP TPs,
...                 ADR-001 URN vocabulary). 5.12 pp. 241-242: the query
...                 side contributes "the selector of Entity Types (if
...                 present), the list of Entity identifiers (if present),
...                 the id pattern (if present)"; conditions include "At
...                 least one of the specified Entity identifiers matches
...                 the idPattern in the EntityInfo", "The specified id
...                 pattern matches the id in the EntityInfo" and "Both a
...                 specified id pattern and an idPattern in the Entity
...                 Info are present (since in the general case it is not
...                 easily feasible to determine if there can be
...                 identifiers matching both patterns)". 4.3.6.1 p. 40:
...                 "It is the responsibility of the Context Broker to
...                 respect the registration parameters when issuing
...                 distributed requests … Ultimately, all constraints
...                 specified in the registration shall be respected" (the
...                 forwarded-request narrowing). 5.7.2.4 distributed
...                 query; 5.5.13 local=true; 5.10.2 CSR discovery reuses
...                 the clause 5.12 matching.

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


*** Test Cases ***
IOP_EXT_IDR_02_11 A Forwarded Query Is Narrowed To The Ids The CSR Can Match
    [Documentation]    4.3.6.1 ("all constraints specified in the
    ...    registration shall be respected") over 5.7.2.4: the query asks
    ...    id=A,B where only A matches the CSR's anchored pattern — the
    ...    forwarded request carries A and must NOT carry B; B is answered
    ...    locally and the union holds both.
    [Tags]    iop    iop-ext    5_12    5_7_2    4_3_6    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Mock As Idr Source    ${info}
    ${local}=    Waste Entity    ${eid_presov}    ${0.7}
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Waste Entity    ${eid_bb}    ${0.42}
    ${remote_arr}=    Evaluate    [$remote]
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    ${remote_arr}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    id=${eid_presov},${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${eid_bb}
    Should Contain    ${response.text}    ${eid_presov}
    Wait For Request
    ${url}=    Get Request Url
    ${url}=    Evaluate    __import__('urllib.parse', fromlist=['unquote']).unquote($url)
    Should Contain    ${url}    ${eid_bb}
    Should Not Contain    ${url}    ${eid_presov}

IOP_EXT_IDR_02_12 A Query IdPattern Matches An Exact EntityInfo Id
    [Documentation]    5.12 condition 4: "The specified id pattern matches
    ...    the id in the EntityInfo" — the CSR registers one exact id, the
    ...    query supplies a pattern matching that id, and the query is
    ...    forwarded (the entity lives only in B2).
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    idPattern=${pat_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${eid_bb}

IOP_EXT_IDR_02_13 Two Patterns Are Assumed Compatible And Forwarded
    [Documentation]    5.12 condition 5: "Both a specified id pattern and
    ...    an idPattern in the Entity Info are present (since in the
    ...    general case it is not easily feasible to determine if there
    ...    can be identifiers matching both patterns)" — even patterns
    ...    over different razidlos must forward; the zvolen entity lives
    ...    only in B2 behind a sk_banskabystrica-pattern CSR and is still
    ...    found.
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_zvolen}    ${0.3}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    idPattern=${pat_zvolen}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${eid_zvolen}

IOP_EXT_IDR_02_14 A Foreign-Razidlo Query Pattern Vs An Exact CSR Id Never Forwards
    [Documentation]    5.12: with the CSR restricted to one exact id, a
    ...    query idPattern anchored to another razidlo satisfies NO match
    ...    condition — the source records ZERO requests and the response
    ...    carries no warning. Positive control: a pattern matching the
    ...    registered id IS forwarded to the same mock.
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    Register Mock As Idr Source    ${info}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    idPattern=${pat_presov}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.text}    []
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    []
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    idPattern=${pat_bb}
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_02_15 A Type-Only Query Cannot Be Excluded By An Id-Restricted CSR
    [Documentation]    5.12: a type-only query specifies no Entity
    ...    identifiers and no id pattern, so the EntityInfo's id
    ...    restriction cannot rule the registration out — the query is
    ...    forwarded and the remote-only entity is returned (current
    ...    Antares behaviour: an id-restricted EntityInfo matches when the
    ...    query side gives no id restriction).
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${eid_bb}

IOP_EXT_IDR_02_16 Razidlo Fan-Out Merges To The Exact Union
    [Documentation]    5.7.2.4 + 4.5.5: three brokers, one razidlo each
    ...    (B1 local sk_presov, B2 sk_banskabystrica, B3 sk_zvolen) —
    ...    a type query via B1 returns exactly the three-entity union,
    ...    every id exactly once, no duplicates.
    [Tags]    iop    iop-ext    5_12    5_7_2    4_5_5    since_v1.9.1
    ${info_bb}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info_bb}
    ${info_zv}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_zvolen}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ...    information=${info_zv}
    ${e1}=    Waste Entity    ${eid_presov}    ${0.7}
    Create Entity At Broker    ${b1_url}    ${e1}
    ${e2}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e2}
    ${e3}=    Waste Entity    ${eid_zvolen}    ${0.3}
    Create Entity At Broker    ${b3_url}    ${e3}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    3
    FOR    ${eid}    IN    ${eid_presov}    ${eid_bb}    ${eid_zvolen}
        ${n}=    Evaluate    $response.text.count($eid)
        Should Be Equal As Integers    ${n}    1
    END

IOP_EXT_IDR_02_17 local=true Never Forwards Regardless Of Pattern Match
    [Documentation]    5.5.13: "local=true … no Context Source
    ...    Registrations shall be considered" — the query id matches the
    ...    CSR pattern and the source still records ZERO requests; the
    ...    result is the local data only, without warnings.
    [Tags]    iop    iop-ext    5_12    5_5_13    5_7_2    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Mock As Idr Source    ${info}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    id=${eid_bb}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.text}    []
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

IOP_EXT_IDR_02_18 A Dark Entity Appears Only In The Federated View
    [Documentation]    4.3.6.2: the entity exists ONLY behind the CSR (in
    ...    B2) — the plain query via B1 includes it, the local=true query
    ...    must NOT (negative half of the same surface).
    [Tags]    iop    iop-ext    5_12    5_7_2    4_3_6    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${eid_bb}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${eid_bb}

IOP_EXT_IDR_02_19 CSR Discovery Filters By Id And IdPattern Per Clause 5.12
    [Documentation]    5.10.2 reuses the clause 5.12 matching: discovery
    ...    with ?id=<urn> returns only the CSR whose EntityInfo.id equals
    ...    it; discovery with ?idPattern= matches the exact-id CSR
    ...    (pattern-vs-id condition) AND the pattern CSR (both-patterns
    ...    condition); a different-type CSR never appears.
    [Tags]    iop    iop-ext    5_12    5_10_2    since_v1.9.1
    ${info1}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info1}
    ${info2}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_zvolen}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b2_url}    ${etype}
    ...    information=${info2}
    ${info3}=    Evaluate    [{"entities": [{"type": $etype + "Other"}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b2_url}    ${etype}Other
    ...    information=${info3}

    ${response}=    GET    url=${b1_url}/csourceRegistrations
    ...    params=type=${etype}&id=${eid_bb}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${registration_id}
    Should Not Contain    ${response.text}    ${registration_id}-2
    Should Not Contain    ${response.text}    ${registration_id}-3

    ${response}=    GET    url=${b1_url}/csourceRegistrations
    ...    params=type=${etype}&idPattern=${pat_bb}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${registration_id}
    Should Contain    ${response.text}    ${registration_id}-2
    Should Not Contain    ${response.text}    ${registration_id}-3


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    WasteC${suffix}
    Set Test Variable    ${base}    urn:ngsi-ld:WasteC${suffix}
    ${pat_bb}=    Evaluate    "^" + $base + ":sk_banskabystrica:odpady:.*$"
    Set Test Variable    ${pat_bb}
    ${pat_zvolen}=    Evaluate    "^" + $base + ":sk_zvolen:odpady:.*$"
    Set Test Variable    ${pat_zvolen}
    ${pat_presov}=    Evaluate    "^" + $base + ":sk_presov:odpady:.*$"
    Set Test Variable    ${pat_presov}
    Set Test Variable    ${eid_bb}    ${base}:sk_banskabystrica:odpady:kontajner:0042
    Set Test Variable    ${eid_presov}    ${base}:sk_presov:odpady:kontajner:0001
    Set Test Variable    ${eid_zvolen}    ${base}:sk_zvolen:odpady:kontajner:0007
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopidr-${suffix}
    Set Test Variable    ${server_started}    ${False}

Waste Entity
    [Documentation]    ADR-001-shaped WasteContainer fixture.
    [Arguments]    ${eid}    ${level}
    ${e}=    Evaluate
    ...    {"id": $eid, "type": $etype, "fillLevel": {"type": "Property", "value": $level}}
    RETURN    ${e}

Register Mock As Idr Source
    [Arguments]    ${info}    ${rid}=${EMPTY}
    ${rid}=    Set Variable If    '${rid}' == ''    ${registration_id}    ${rid}
    ${reg}=    Evaluate
    ...    {"id": $rid, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

Start Mock
    Start Server    ${mock_host}    ${mock_port}
    Set Test Variable    ${server_started}    ${True}

Cleanup Interop Fixtures
    FOR    ${rid}    IN    ${registration_id}    ${registration_id}-2    ${registration_id}-3
        Delete Registration At Broker    ${b1_url}    ${rid}
    END
    FOR    ${eid}    IN    ${eid_bb}    ${eid_presov}    ${eid_zvolen}
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
        Delete Entity Via Broker    ${b3_url}    ${eid}
    END
    IF    ${server_started}
        Stop Server
    END
