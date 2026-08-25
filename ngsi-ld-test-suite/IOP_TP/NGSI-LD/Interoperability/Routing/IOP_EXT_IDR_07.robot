*** Settings ***
Documentation       Negative routing — the spec's must-NOT-forward surface
...                 (Antares extension IOP TPs). Verbatim grounds: 4.3.6.1
...                 p. 41 — brokers "shall respect" a source's limited
...                 operations "to avoid unnecessarily sending distributed
...                 operation requests which are always guaranteed to
...                 fail"; 4.3.6.2 — auxiliary registrations "are limited
...                 to context information consumption operations (see
...                 clause 5.7)"; 4.3.6.4 — "no registration shall match
...                 if the CSourceRegistration contextSourceAlias can be
...                 found within the listing of previously encountered
...                 Context Sources" and "each Tenant … shall be
...                 considered separately"; 5.12 — the attribute-overlap
...                 conditions ("The combination of Properties and
...                 Relationships is empty … means only Entities have
...                 been registered" ⇒ match) and the datasetId
...                 common-value condition ("should only be considered
...                 matching, if both have at least one value in common.
...                 If only one of them specifies a datasetId, it is
...                 considered a match" — should-level). Every case
...                 asserts the mock records ZERO requests: the routing
...                 decision itself is the subject under test.

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
IOP_EXT_IDR_07_51 An Attribute-Scope Mismatch Blocks The Forward
    [Documentation]    5.12 attribute conditions: the RegistrationInfo
    ...    lists only fillLevel; a query asking ?attrs= of a different
    ...    attribute cannot match it — ZERO requests at the source even
    ...    though the idPattern matches. Control: attrs=fillLevel IS
    ...    forwarded.
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    Start Mock
    ${info}=    Evaluate
    ...    [{"entities": [{"type": $etype, "idPattern": $pat_bb}], "propertyNames": ["fillLevel"]}]
    Register Mock As Idr Source    ${info}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    attrs=rychlost
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.text}    []
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

    ${remote}=    Evaluate
    ...    [{"id": $eid_bb, "type": $etype, "fillLevel": {"type": "Property", "value": 0.4}}]
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    ${remote}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    attrs=fillLevel
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_07_52 An Entities-Only RegistrationInfo Matches Any Attrs
    [Documentation]    5.12 over-pruning guard: "The combination of
    ...    Properties and Relationships is empty (as this means only
    ...    Entities have been registered and the Context Sources may have
    ...    matching Property or Relationship instances)" — a
    ...    propertyNames-less RegistrationInfo MUST still be forwarded a
    ...    query with any ?attrs=.
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Mock As Idr Source    ${info}
    ${remote}=    Evaluate
    ...    [{"id": $eid_bb, "type": $etype, "lubovolnyAtribut": {"type": "Property", "value": 1}}]
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    ${remote}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    attrs=lubovolnyAtribut
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_07_53 Disjoint DatasetIds Block The Forward; One-Sided Matches
    [Documentation]    5.12 (should-level, noted): "If the request …
    ...    includes a datasetId parameter and the CSourceRegistration …
    ...    contains a datasetId element, the CSourceRegistration should
    ...    only be considered matching, if both have at least one value
    ...    in common. If only one of them specifies a datasetId, it is
    ...    considered a match." Disjoint → ZERO requests; only-one-side →
    ...    forwarded.
    [Tags]    iop    iop-ext    5_12    5_7_2    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port), "datasetId": ["urn:ngsi-ld:Dataset:b"]}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ...    datasetId=urn:ngsi-ld:Dataset:a
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    []
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_07_54 An Expired Registration Never Matches; Offset Timestamps Are Rejected
    [Documentation]    5.2.9/5.9.2.4: once expiresAt is reached the
    ...    registration "counts as deleted" — the retrieve stops dialing
    ...    the source (count frozen at the pre-expiry control) and
    ...    discovery omits it. Edge (pins the audited string-compare
    ...    finding): 4.6.3 mandates the Z form — an expiresAt carrying a
    ...    "+01:00" offset is 400 BadRequestData, so a lexically
    ...    misordered offset timestamp can never enter the expiry
    ...    comparison.
    [Tags]    iop    iop-ext    5_2_9    5_9_2    4_6_3    5_12    since_v1.9.1
    Start Mock
    ${remote}=    Waste Entity    ${eid_bb}    ${0.4}
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${eid_bb}    200    ${remote}
    ${expires}=    Evaluate
    ...    (__import__('datetime').datetime.utcnow() + __import__('datetime').timedelta(seconds=3)).strftime('%Y-%m-%dT%H:%M:%SZ')
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port), "expiresAt": $expires}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Sleep    4s
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    404    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities/${eid_bb}
    Should Be Equal As Integers    ${hits}    1
    ${response}=    GET    url=${b1_url}/csourceRegistrations    params=type=${etype}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${registration_id}

    ${reg}=    Evaluate
    ...    {"id": $registration_id + "-2", "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port), "expiresAt": "2030-01-01T00:00:00+01:00"}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    400    ${response.status_code}
    Should Contain    ${response.text}    BadRequestData

IOP_EXT_IDR_07_55 A Provision-Only Source Is Never Dialed For Reads
    [Documentation]    4.3.6.1 p. 41 ("shall respect this, to avoid
    ...    unnecessarily sending distributed operation requests which are
    ...    always guaranteed to fail") + 4.20: the CSR offers ONLY
    ...    createEntity — neither the retrieve nor the query may dial it
    ...    even though the idPattern matches: ZERO requests, no warning.
    [Tags]    iop    iop-ext    4_3_6    4_20    5_12    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "operations": ["createEntity"], "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.text}    []
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

IOP_EXT_IDR_07_56 An Auxiliary Source Never Receives Provision Operations
    [Documentation]    4.3.6.2: auxiliary registrations "are limited to
    ...    context information consumption operations (see clause 5.7)" —
    ...    create, attribute update and delete with a matching id stay
    ...    local and the auxiliary mock records ZERO requests; the SAME
    ...    auxiliary CSR is then consulted for the retrieve (both halves).
    [Tags]    iop    iop-ext    4_3_6    5_12    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "auxiliary", "operations": ["retrieveOps"], "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b1_url}    ${e}
    ${patch}=    Evaluate    {"fillLevel": {"type": "Property", "value": 0.5}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${eid_bb}    ${patch}
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Delete Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    204    ${response.status_code}
    Wait For No Request    ${2}

    ${remote}=    Waste Entity    ${eid_bb}    ${0.9}
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${eid_bb}    200    ${remote}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities/${eid_bb}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_07_57 A Registration In Tenant A Never Routes Tenant B's Request
    [Documentation]    4.3.6.4 ("each Tenant … shall be considered
    ...    separately") + 4.14: the CSR lives in tenant mestoA — tenant
    ...    mestoB's retrieve of a matching id is a clean 404 with NO
    ...    warning and ZERO requests at the source; mestoA's own retrieve
    ...    (control) is forwarded.
    [Tags]    iop    iop-ext    4_3_6    4_14    5_12    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    &{headers}=    Create Dictionary    Content-Type=application/json    NGSILD-Tenant=mestoa
    ${response}=    POST    url=${b1_url}/csourceRegistrations    json=${reg}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    mestob
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning
    Wait For No Request    ${2}

    ${remote}=    Waste Entity    ${eid_bb}    ${0.4}
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${eid_bb}    200    ${remote}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}    mestoa
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities/${eid_bb}
    Should Be Equal As Integers    ${hits}    1

IOP_EXT_IDR_07_58 A Previously Encountered Alias Excludes The Registration
    [Documentation]    4.3.6.4: "no registration shall match if the
    ...    CSourceRegistration contextSourceAlias can be found within the
    ...    listing of previously encountered Context Sources" — a request
    ...    arriving WITH a Via chain naming the CSR's alias is answered
    ...    404 with ZERO re-forwards; the same request without the chain
    ...    (control) dials the source.
    [Tags]    iop    iop-ext    4_3_6    6_3_18    5_12    since_v1.9.1
    Start Mock
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port), "contextSourceAlias": "zdroj-bb-${suffix}"}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($eid_bb, safe='')
    &{headers}=    Create Dictionary    Via=1.1 zdroj-bb-${suffix}
    ${response}=    GET    url=${b1_url}/entities/${eid}    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    Wait For No Request    ${2}

    ${remote}=    Waste Entity    ${eid_bb}    ${0.4}
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${eid_bb}    200    ${remote}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities/${eid_bb}
    Should Be Equal As Integers    ${hits}    1


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    WasteC${suffix}
    Set Test Variable    ${base}    urn:ngsi-ld:WasteC${suffix}
    ${pat_bb}=    Evaluate    "^" + $base + ":sk_banskabystrica:odpady:.*$"
    Set Test Variable    ${pat_bb}
    Set Test Variable    ${eid_bb}    ${base}:sk_banskabystrica:odpady:kontajner:0042
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
    FOR    ${rid}    IN    ${registration_id}    ${registration_id}-2
        Delete Registration At Broker    ${b1_url}    ${rid}
    END
    ${rid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($registration_id, safe='')
    &{headers}=    Create Dictionary    NGSILD-Tenant=mestoa
    ${response}=    DELETE    url=${b1_url}/csourceRegistrations/${rid}    headers=${headers}    expected_status=any
    FOR    ${eid}    IN    ${eid_bb}
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
    END
    IF    ${server_started}
        Stop Server
    END
