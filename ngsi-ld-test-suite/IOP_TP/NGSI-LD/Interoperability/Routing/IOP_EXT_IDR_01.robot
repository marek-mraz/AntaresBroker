*** Settings ***
Documentation       Retrieve-by-id routing via EntityInfo id/idPattern
...                 (Antares extension IOP TPs, ADR-001 URN vocabulary).
...                 5.12: an Entity specification "matches an EntityInfo
...                 element of the RegistrationInfo if the type selector
...                 matches the entity types in the EntityInfo element and
...                 one of the following conditions holds: The EntityInfo
...                 contains neither an id nor an idPattern; One of the
...                 specified Entity identifiers matches the id in the
...                 EntityInfo; At least one of the specified Entity
...                 identifiers matches the idPattern in the EntityInfo;
...                 The specified id pattern matches the id in the
...                 EntityInfo; Both a specified id pattern and an
...                 idPattern in the Entity Info are present." 5.2.8:
...                 idPattern is a "Regular expression as per IEEE 1003.2"
...                 — matching is find/substring semantics unless the
...                 pattern anchors itself, which is why ADR-001 mandates
...                 the ^...$ anchored prefix form. 4.3.6.1: brokers avoid
...                 "unnecessarily sending distributed operation requests
...                 which are always guaranteed to fail" — a CSR whose
...                 idPattern cannot match the requested id is never
...                 dialed (the pruning claim; asserted with zero-hit
...                 mocks). 5.7.1.4 retrieve forwarding; 6.3.17 warnings.

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
${mock_host}        127.0.0.1
${mock_port}        8089


*** Test Cases ***
IOP_EXT_IDR_01_01 Anchored ADR Prefix Pattern Routes Retrieve To The Owning Source
    [Documentation]    5.12 + 5.7.1.4: a CSR whose EntityInfo carries the
    ...    ADR-001 anchored razidlo prefix pattern routes
    ...    GET /entities/{id} to the ONE owning source — the entity exists
    ...    only in B2 (dark entity) yet retrieves via B1; local=true must
    ...    still be 404 (5.5.13 local scope excludes forwarding).
    [Tags]    iop    iop-ext    5_12    5_7_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_bb}
    Should Contain    ${response.text}    fillLevel

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}
    Should Not Contain    ${response.text}    fillLevel

IOP_EXT_IDR_01_02 A Non-Matching Razidlo Is Never Dialed
    [Documentation]    5.12/4.3.6.1 pruning: the requested id carries
    ...    razidlo sk_presov, the CSR pattern anchors sk_banskabystrica —
    ...    the pattern cannot match, so B1 answers 404 WITHOUT contacting
    ...    the source (zero requests at the mock) and WITHOUT an
    ...    NGSILD-Warning (6.3.17 warnings mark failed *attempts*, and no
    ...    attempt may be made). Positive control: a matching id IS
    ...    forwarded to the same mock.
    [Tags]    iop    iop-ext    5_12    5_7_1    6_3_17    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Mock As Idr Source    ${info}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_presov}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

    ${remote}=    Waste Entity    ${eid_bb}    ${0.42}
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${eid_bb}    200    ${remote}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities/${eid_bb}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_01_03 An Exact EntityInfo Id Routes Only That One Id
    [Documentation]    5.12 condition 2 ("One of the specified Entity
    ...    identifiers matches the id in the EntityInfo"): the CSR names
    ...    one exact id — that id routes to B2; a sibling id under the
    ...    same prefix is NOT forwarded, so it 404s even though B2 holds
    ...    it.
    [Tags]    iop    iop-ext    5_12    5_7_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e1}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e1}
    ${e2}=    Waste Entity    ${eid_sib}    ${0.13}
    Create Entity At Broker    ${b2_url}    ${e2}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_sib}
    Check Response Status Code    404    ${response.status_code}
    Should Not Contain    ${response.text}    fillLevel

IOP_EXT_IDR_01_04 An Unanchored Pattern Matches By Substring
    [Documentation]    5.2.8 (IEEE 1003.2 regex) + 5.12: an idPattern
    ...    without ^...$ anchors matches by regex find — an id carrying
    ...    the pattern text mid-URN is routed. This is the divergence
    ...    surface ADR-001 neutralizes by mandating anchored patterns:
    ...    unanchored prefixes over-match.
    [Tags]    iop    iop-ext    5_12    5_2_8    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": "sk_banskabystrica:odpady"}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_mid}    ${0.9}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_mid}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_mid}

IOP_EXT_IDR_01_05 A Type-Only EntityInfo Matches Every Id Of That Type
    [Documentation]    5.12 condition 1: "The EntityInfo contains neither
    ...    an id nor an idPattern" — every id of the registered type is
    ...    routed, across different razidlos.
    [Tags]    iop    iop-ext    5_12    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e1}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e1}
    ${e2}=    Waste Entity    ${eid_presov}    ${0.7}
    Create Entity At Broker    ${b2_url}    ${e2}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_presov}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_presov}

IOP_EXT_IDR_01_06 The EntityInfo Type Selector Gates A Matching IdPattern
    [Documentation]    5.12: an EntityInfo matches only "if the type
    ...    selector matches the entity types in the EntityInfo element AND
    ...    one of the following conditions holds" — a query whose type
    ...    differs from the EntityInfo type is NOT forwarded even though
    ...    the idPattern matches the requested id (zero requests at the
    ...    mock, no warning). Positive control: the registered type IS
    ...    forwarded.
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Mock As Idr Source    ${info}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}Other    id=${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.text}    []
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    []
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    id=${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_01_07 Any-Of Matching Across Multiple EntityInfo Elements
    [Documentation]    5.12: a RegistrationInfo matches if the specification
    ...    matches "at least one of the EntityInfo elements" — the
    ...    requested id matches only the SECOND EntityInfo element and the
    ...    registration still routes.
    [Tags]    iop    iop-ext    5_12    5_7_1    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"type": $etype, "idPattern": $pat_zvolen}, {"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_bb}

IOP_EXT_IDR_01_08 Disjoint Razidlo CSRs: Exactly One Endpoint Is Dialed
    [Documentation]    5.12 + 4.3.6.2: two CSRs with disjoint anchored
    ...    razidlo prefixes — the retrieve dials EXACTLY the matching
    ...    endpoint (B2 answers) while the non-matching source records
    ...    zero requests. Federation cost scales with matching CSRs, not
    ...    total CSRs (the ADR-001 pruning claim).
    [Tags]    iop    iop-ext    5_12    5_7_1    since_v1.9.1
    Start Mock
    ${info_bb}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info_bb}
    ${info_zv}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_zvolen}]}]
    Register Mock As Idr Source    ${info_zv}    ${registration_id}-2
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_bb}
    Wait For No Request    ${2}

IOP_EXT_IDR_01_09 An Invalid Regex In IdPattern Is Rejected At Registration
    [Documentation]    5.2.8: idPattern shall be a valid IEEE 1003.2
    ...    regular expression — an unparseable pattern is 400
    ...    BadRequestData and NOTHING is registered (the follow-up GET of
    ...    the registration id is 404).
    [Tags]    iop    iop-ext    5_2_8    5_9_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": "[invalid"}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    400    ${response.status_code}
    Should Contain    ${response.text}    BadRequestData

    ${response}=    Get Registration At Broker    ${b1_url}    ${registration_id}
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_IDR_01_10 Pattern Matching Is Case-Sensitive
    [Documentation]    5.2.8 IEEE 1003.2 regexes are case-sensitive (and
    ...    ADR-001 mandates lowercase razidlos): the lowercase anchored
    ...    pattern must NOT match an id whose razidlo is spelled
    ...    SK_BanskaBystrica — that id 404s via B1 even though B2 holds
    ...    it, while the lowercase id routes.
    [Tags]    iop    iop-ext    5_12    5_2_8    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${upper}=    Waste Entity    ${eid_upper}    ${0.5}
    Create Entity At Broker    ${b2_url}    ${upper}
    ${lower}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${lower}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_upper}
    Check Response Status Code    404    ${response.status_code}
    Should Not Contain    ${response.text}    fillLevel
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_bb}


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    WasteC${suffix}
    Set Test Variable    ${base}    urn:ngsi-ld:WasteC${suffix}
    ${pat_bb}=    Evaluate    "^" + $base + ":sk_banskabystrica:odpady:.*$"
    Set Test Variable    ${pat_bb}
    ${pat_zvolen}=    Evaluate    "^" + $base + ":sk_zvolen:odpady:.*$"
    Set Test Variable    ${pat_zvolen}
    Set Test Variable    ${eid_bb}    ${base}:sk_banskabystrica:odpady:kontajner:0042
    Set Test Variable    ${eid_sib}    ${base}:sk_banskabystrica:odpady:kontajner:0002
    Set Test Variable    ${eid_presov}    ${base}:sk_presov:odpady:kontajner:0001
    Set Test Variable    ${eid_mid}    ${base}:oblast:sk_banskabystrica:odpady:0009
    Set Test Variable    ${eid_upper}    ${base}:SK_BanskaBystrica:odpady:kontajner:0001
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
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-2
    FOR    ${eid}    IN    ${eid_bb}    ${eid_sib}    ${eid_presov}    ${eid_mid}    ${eid_upper}
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
    END
    IF    ${server_started}
        Stop Server
    END
