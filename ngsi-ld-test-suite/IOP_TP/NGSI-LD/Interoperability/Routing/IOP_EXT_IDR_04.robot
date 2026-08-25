*** Settings ***
Documentation       Multi-broker id/idPattern routing topologies over the
...                 five-broker fleet (Antares extension IOP TPs, ADR-001
...                 URN vocabulary). 4.3.6.4: cascading distributed
...                 operations are limited via "a specific field listing
...                 all previously encountered Context Sources (e.g. a Via
...                 header …) shall be passed as part of the request and
...                 this field can be used to exclude duplicated sources
...                 from matching"; localOnly requests "act only on data
...                 held directly by the registered Context Source
...                 itself". 4.3.6.3: "In the case that multiple
...                 overlapping redirect registrations are defined,
...                 operations are distributed to all registered Context
...                 Sources." 5.2.34: timeout = "Maximum period of time in
...                 milliseconds which may elapse before a forwarded
...                 request is assumed to have failed"; cooldown = "If
...                 requests are received before the cooldown period has
...                 expired, a timeout error response for the registration
...                 is automatically returned." 6.3.17: a 404 source is
...                 not abnormal (no warning); an unreachable matched
...                 source is warning 199. 5.9.4 delete registration.

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
${b4_url}
${b5_url}
${mock_host}        127.0.0.1
${mock_port}        8089
${dead_endpoint}    http://127.0.0.1:59999


*** Test Cases ***
IOP_EXT_IDR_04_30 A Four-Spoke Star Routes Each Retrieve To Its One Owner
    [Documentation]    5.7.1.4 + 5.12: B1 carries four disjoint anchored
    ...    razidlo CSRs (B2..B5); each entity holds a per-broker marker.
    ...    Retrieves via B1 return the owning broker's marker and NEVER a
    ...    foreign broker's marker.
    [Tags]    iop    iop-ext    5_12    5_7_1    since_v1.9.1
    Register Star
    Seed Star Entities

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['zdroj']['value']}    B2
    FOR    ${foreign}    IN    B3    B4    B5
        Should Not Contain    ${response.text}    "${foreign}"
    END
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_zvolen}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['zdroj']['value']}    B3
    Should Not Contain    ${response.text}    "B2"

IOP_EXT_IDR_04_31 The Star Type Query Is The Exact Union Of All Spokes
    [Documentation]    5.7.2.4: the type query via B1 returns the four
    ...    remote razidlo entities plus B1's own local one — exactly five,
    ...    every id exactly once.
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    Register Star
    Seed Star Entities
    ${local}=    Waste Entity    ${eid_martin}    ${0.1}    B1
    Create Entity At Broker    ${b1_url}    ${local}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    5
    FOR    ${eid}    IN    ${eid_bb}    ${eid_zvolen}    ${eid_presov}    ${eid_kosice}    ${eid_martin}
        ${n}=    Evaluate    $response.text.count($eid)
        Should Be Equal As Integers    ${n}    1
    END

IOP_EXT_IDR_04_32 A Cascade Resolves Through The Narrower Downstream Prefix
    [Documentation]    4.3.6.4: B1 registers the coarse razidlo prefix at
    ...    B2; B2 registers the narrower odpady prefix at B3; the entity
    ...    lives ONLY at B3 — the retrieve via B1 resolves through the
    ...    chain (and B2 holds nothing locally: negative half).
    [Tags]    iop    iop-ext    5_12    4_3_6    5_7_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_coarse}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}    B3
    Create Entity At Broker    ${b3_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['zdroj']['value']}    B3
    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_IDR_04_33 An Overlapping Registration Loop Terminates Via The Via Chain
    [Documentation]    4.3.6.4 + 6.3.18: B1 registers the pattern at B2 and
    ...    B2 registers the same pattern back at B1 — the Via listing of
    ...    previously encountered Context Sources excludes the duplicate
    ...    source from matching, the request terminates and still returns
    ...    B2's data.
    [Tags]    iop    iop-ext    5_12    4_3_6    6_3_18    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2    ${b1_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}    B2
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_bb}
    Should Be Equal    ${response.json()['zdroj']['value']}    B2

IOP_EXT_IDR_04_34 localOnly Stops The Cascade At The First Hop
    [Documentation]    4.3.6.4/5.2.34 localOnly: distributed operations
    ...    "act only on data held directly by the registered Context
    ...    Source itself" — B1's localOnly CSR reaches B2, but B2 must NOT
    ...    cascade to its own B3 registration, so the B3-only entity is
    ...    invisible via B1 while B2's own retrieve (control) cascades and
    ...    finds it.
    [Tags]    iop    iop-ext    5_12    4_3_6    5_2_34    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_coarse}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}    local_only=${True}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}    B3
    Create Entity At Broker    ${b3_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    404    ${response.status_code}
    Should Not Contain    ${response.text}    fillLevel
    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['zdroj']['value']}    B3

IOP_EXT_IDR_04_35 Overlapping Redirect Registrations Distribute To All
    [Documentation]    4.3.6.3: "In the case that multiple overlapping
    ...    redirect registrations are defined, operations are distributed
    ...    to all registered Context Sources" — a create via B1 whose id
    ...    both redirect patterns match lands at B2 AND B3, and is never
    ...    held at B1.
    [Tags]    iop    iop-ext    5_12    4_3_6    5_6_1    since_v1.9.1
    ${endpoint2}=    Broker Base Of    ${b2_url}
    ${endpoint3}=    Broker Base Of    ${b3_url}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "operations": ["redirectionOps"], "information": $info, "endpoint": $endpoint2}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}
    ${reg}=    Evaluate
    ...    {"id": $registration_id + "-2", "type": "ContextSourceRegistration", "mode": "redirect", "operations": ["redirectionOps"], "information": $info, "endpoint": $endpoint3}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${e}=    Waste Entity    ${eid_bb}    ${0.42}    B1
    Create Entity At Broker    ${b1_url}    ${e}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b3_url}    ${eid_bb}    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_IDR_04_36 Prefix Shadowing Consults Both The Coarse And The Fine Source
    [Documentation]    5.12 + 4.5.5: a coarse-prefix CSR (B2) and a
    ...    fine-prefix CSR (B3) BOTH match the id — both are consulted and
    ...    the split attributes merge into one entity.
    [Tags]    iop    iop-ext    5_12    4_5_5    5_7_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_coarse}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}    B2
    Create Entity At Broker    ${b2_url}    ${e}
    ${frag}=    Evaluate
    ...    {"id": $eid_bb, "type": $etype, "status": {"type": "Property", "value": "plny"}}
    Create Entity At Broker    ${b3_url}    ${frag}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    fillLevel
    Should Contain    ${response.text}    plny

IOP_EXT_IDR_04_37 A Dead Matched Endpoint Warns; A Non-Matching Id Does Not
    [Documentation]    6.3.17: the pattern-matching id against an
    ...    unreachable registered source is 404 WITH NGSILD-Warning (199
    ...    class — no response within the timeout); a non-matching id is a
    ...    clean 404 WITHOUT any warning because the source was never
    ...    dialed (pins the 2026-08-15 live verification).
    [Tags]    iop    iop-ext    5_12    6_3_17    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $dead_endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-Warning
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_presov}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning

IOP_EXT_IDR_04_38 The Cooldown Fails Fast Without Re-Dialing The Source
    [Documentation]    5.2.34: after a timeout failure, "If requests are
    ...    received before the cooldown period has expired, a timeout
    ...    error response for the registration is automatically returned"
    ...    — the second retrieve inside the cooldown still fails (404 +
    ...    warning) and the source records EXACTLY ONE dial.
    [Tags]    iop    iop-ext    5_2_34    6_3_17    since_v1.9.1
    Start Mock
    ${management}=    Evaluate    {"timeout": 1500, "cooldown": 30000}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port), "management": $management}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-Warning
    Wait And Ignore Request

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

IOP_EXT_IDR_04_39 Routing Follows The Live Registration Set
    [Documentation]    5.9.4 + 5.12: while the CSR exists the retrieve is
    ...    served by the source (stub count 1); after DELETE
    ...    /csourceRegistrations/{id} the same retrieve is 404 and the
    ...    source records ZERO new hits.
    [Tags]    iop    iop-ext    5_12    5_9_4    since_v1.9.1
    Start Mock
    ${remote}=    Waste Entity    ${eid_bb}    ${0.42}    MOCK
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${eid_bb}    200    ${remote}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Mock As Idr Source    ${info}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities/${eid_bb}
    Should Be Equal As Integers    ${hits}    1

    ${response}=    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    404    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities/${eid_bb}
    Should Be Equal As Integers    ${hits}    1


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    WasteC${suffix}
    Set Test Variable    ${base}    urn:ngsi-ld:WasteC${suffix}
    ${pat_coarse}=    Evaluate    "^" + $base + ":sk_banskabystrica:.*$"
    Set Test Variable    ${pat_coarse}
    ${pat_bb}=    Evaluate    "^" + $base + ":sk_banskabystrica:odpady:.*$"
    Set Test Variable    ${pat_bb}
    ${pat_zvolen}=    Evaluate    "^" + $base + ":sk_zvolen:odpady:.*$"
    Set Test Variable    ${pat_zvolen}
    ${pat_presov}=    Evaluate    "^" + $base + ":sk_presov:odpady:.*$"
    Set Test Variable    ${pat_presov}
    ${pat_kosice}=    Evaluate    "^" + $base + ":sk_kosice:odpady:.*$"
    Set Test Variable    ${pat_kosice}
    Set Test Variable    ${eid_bb}    ${base}:sk_banskabystrica:odpady:kontajner:0042
    Set Test Variable    ${eid_zvolen}    ${base}:sk_zvolen:odpady:kontajner:0007
    Set Test Variable    ${eid_presov}    ${base}:sk_presov:odpady:kontajner:0001
    Set Test Variable    ${eid_kosice}    ${base}:sk_kosice:odpady:kontajner:0011
    Set Test Variable    ${eid_martin}    ${base}:sk_martin:odpady:kontajner:0003
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopidr-${suffix}
    Set Test Variable    ${server_started}    ${False}

Waste Entity
    [Documentation]    ADR-001-shaped WasteContainer fixture with a
    ...    per-broker origin marker (zdroj).
    [Arguments]    ${eid}    ${level}    ${marker}=X
    ${e}=    Evaluate
    ...    {"id": $eid, "type": $etype, "fillLevel": {"type": "Property", "value": $level}, "zdroj": {"type": "Property", "value": $marker}}
    RETURN    ${e}

Register Star
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_zvolen}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ...    information=${info}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_presov}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b4_url}    ${etype}
    ...    information=${info}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_kosice}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}-4    ${b5_url}    ${etype}
    ...    information=${info}

Seed Star Entities
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}    B2
    Create Entity At Broker    ${b2_url}    ${e}
    ${e}=    Waste Entity    ${eid_zvolen}    ${0.3}    B3
    Create Entity At Broker    ${b3_url}    ${e}
    ${e}=    Waste Entity    ${eid_presov}    ${0.7}    B4
    Create Entity At Broker    ${b4_url}    ${e}
    ${e}=    Waste Entity    ${eid_kosice}    ${0.5}    B5
    Create Entity At Broker    ${b5_url}    ${e}

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
    FOR    ${rid}    IN    ${registration_id}    ${registration_id}-2    ${registration_id}-3    ${registration_id}-4
        Delete Registration At Broker    ${b1_url}    ${rid}
    END
    Delete Registration At Broker    ${b2_url}    ${registration_id}-2
    FOR    ${eid}    IN    ${eid_bb}    ${eid_zvolen}    ${eid_presov}    ${eid_kosice}    ${eid_martin}
        FOR    ${broker}    IN    ${b1_url}    ${b2_url}    ${b3_url}    ${b4_url}    ${b5_url}
            Delete Entity Via Broker    ${broker}    ${eid}
        END
    END
    IF    ${server_started}
        Stop Server
    END
