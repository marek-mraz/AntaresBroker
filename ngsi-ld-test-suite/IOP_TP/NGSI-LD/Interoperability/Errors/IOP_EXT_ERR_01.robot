*** Settings ***
Documentation       Errors, timeouts, resilience (Antares extension IOP
...                 TPs). 6.3.17 Table 6.3.17-1: 199 "No response was
...                 received from the registration endpoint within the
...                 specified timeout period or a registration loop has
...                 been detected"; 299 "An error response (such as 403 -
...                 Forbidden) was received"; 111 "the payload of the
...                 response was invalid"; "invalid non-NGSI-LD payloads
...                 shall be rejected and not incorporated into the overall
...                 response"; a 404 source is not abnormal. 5.2.34:
...                 timeout = "Maximum period of time in milliseconds which
...                 may elapse before a forwarded request is assumed to
...                 have failed"; cooldown = "Minimum period of time in
...                 milliseconds which shall elapse before attempting to
...                 make a subsequent forwarded request to the same
...                 endpoint after failure. If requests are received before
...                 the cooldown period has expired, a timeout error
...                 response for the registration is automatically
...                 returned." 5.2.18/5.2.19: UpdateResult = updated
...                 String[] + notUpdated NotUpdatedDetails[]
...                 (attributeName, reason, optional registrationId).

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
IOP_EXT_ERR_01_01 A Peer's 500 Body Never Pollutes The Union
    [Documentation]    6.3.17: an error response from the registration
    ...    endpoint is abnormal behaviour (299) — the union is the local
    ...    data plus a warning, and the peer's error body is rejected, "not
    ...    incorporated into the overall response".
    [Tags]    iop    iop-ext    6_3_17    since_v1.9.1
    Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    500    {"marker": "EVILBODY"}
    Register Mock As Source
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-l
    Should Not Contain    ${response.text}    EVILBODY
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    299

IOP_EXT_ERR_01_02 A Stalling Peer Times Out Into Warning 199
    [Documentation]    6.3.17 Table 6.3.17-1 code 199: "No response was
    ...    received from the registration endpoint within the specified
    ...    timeout period" — a socket that accepts but never answers
    ...    degrades to local data within the deadline budget.
    [Tags]    iop    iop-ext    6_3_17    since_v1.9.1
    Start Mock
    Register Mock As Source
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-l
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    199
    Wait And Ignore Request

IOP_EXT_ERR_01_03 A Redirect-Only Retrieve With A Dead Source Is 404 Plus Warning
    [Documentation]    6.3.17: reads surface abnormal sources as
    ...    NGSILD-Warning (the 508/504/502 list is the unsafe-methods
    ...    paragraph) — with no data anywhere the retrieve ends
    ...    ResourceNotFound, the warning preserved.
    [Tags]    iop    iop-ext    6_3_17    5_7_1    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $dead_endpoint, "operations": ["redirectionOps"]}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    404    ${response.status_code}
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Not Be Empty    ${warning}
    Should Not Contain    ${response.text}    speed

IOP_EXT_ERR_01_04 A Garbage 200 From A Peer Is Warning 111, Never Merged
    [Documentation]    6.3.17: "Although data was received from the
    ...    registration endpoint within the specified timeout period, the
    ...    payload of the response was invalid" (111); "invalid non-NGSI-LD
    ...    payloads shall be rejected and not incorporated".
    [Tags]    iop    iop-ext    6_3_17    since_v1.9.1
    Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    200    GARBAGE-not-json
    Register Mock As Source    id_scoped=${True}
    Create Local Entity
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Kept"}}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    Kept
    Should Not Contain    ${response.text}    GARBAGE
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    111

IOP_EXT_ERR_01_05 management.timeout Bounds The Forward
    [Documentation]    5.2.34: timeout is the "Maximum period of time in
    ...    milliseconds which may elapse before a forwarded request is
    ...    assumed to have failed" — with a 200 ms budget a stalling source
    ...    fails fast into the 199 warning path, well inside the default
    ...    deadline.
    [Tags]    iop    iop-ext    5_2_34    6_3_17    since_v1.9.1
    Start Mock
    Register Mock As Source    management=${{ {"timeout": 200} }}
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}

    ${t0}=    Evaluate    __import__('time').time()
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    ${elapsed}=    Evaluate    __import__('time').time() - $t0
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-l
    Should Be True    ${elapsed} < 4
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    199
    Wait And Ignore Request

IOP_EXT_ERR_01_06 management.cooldown Suppresses Contact After A Failure
    [Documentation]    5.2.34 cooldown: "Minimum period of time in
    ...    milliseconds which shall elapse before attempting to make a
    ...    subsequent forwarded request to the same endpoint after failure.
    ...    If requests are received before the cooldown period has expired,
    ...    a timeout error response for the registration is automatically
    ...    returned" — the mock's hit count stays flat inside the window
    ...    and grows after it.
    [Tags]    iop    iop-ext    5_2_34    since_v1.9.1
    Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    500    {"boom": true}
    Register Mock As Source    management=${{ {"cooldown": 4000} }}
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}

    ${first}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${first.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    1

    ${second}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${second.status_code}
    Should Contain    ${second.text}    ${entity_id}-l
    ${warning}=    Evaluate    $second.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    199
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    1

    Sleep    4.2s
    ${third}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${third.status_code}
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    2

IOP_EXT_ERR_01_07 A Responding-But-Erroring Source Keeps Being Attempted
    [Documentation]    6.3.17 posture (no cooldown declared): failures make
    ...    warnings, but a source that ANSWERS — even with errors — is
    ...    re-attempted on every request; nothing may starve unrelated
    ...    requests to the same host:port.
    [Tags]    iop    iop-ext    6_3_17    6_3_8    since_v1.9.1
    Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    500    {"boom": true}
    Register Mock As Source
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}

    FOR    ${i}    IN RANGE    3
        ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
        Check Response Status Code    200    ${response.status_code}
        Should Contain    ${response.text}    ${entity_id}-l
    END
    ${hits}=    Get Stub Count    GET    /ngsi-ld/v1/entities?type=${etype}
    Should Be Equal As Integers    ${hits}    3

IOP_EXT_ERR_01_08 One Parseable Warning Per Failed Source, None Per Healthy
    [Documentation]    6.3.17/RFC 7234 warn form: each warning value is
    ...    `<code> <agent> "<text>"`; only the failed source contributes
    ...    one — the healthy source contributes none.
    [Tags]    iop    iop-ext    6_3_17    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${dead}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $dead_endpoint}
    ${created}=    Post Registration At Broker    ${b1_url}    ${dead}
    Check Response Status Code    201    ${created.status_code}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b2_url}    ${etype}
    ${r}=    Simple Vehicle Entity    ${entity_id}-r    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-r
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    ${parsed}=    Evaluate
    ...    __import__('re').findall(r'\\d{3} \\S+ "[^"]*"', $warning)
    Length Should Be    ${parsed}    1

IOP_EXT_ERR_01_09 A 403 Source Warns On Reads, The Union Survives
    [Documentation]    6.3.17 Table 6.3.17-1 code 299: "An error response
    ...    (such as 403 - Forbidden) was received from the registration
    ...    endpoint ... insufficient access rights" — on a read the union
    ...    is served with the warning; the 403 never reaches the client
    ...    status.
    [Tags]    iop    iop-ext    6_3_17    since_v1.9.1
    Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    403    {"denied": true}
    Register Mock As Source
    ${l}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${l}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-l
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    299
    Should Not Contain    ${response.text}    denied

IOP_EXT_ERR_01_10 A Purely Remote Delete Succeeds With 204
    [Documentation]    5.6.6.4: the delete forwards to the matching
    ...    redirect registration and "the input data shall be used to
    ...    remove the entity locally IF IT EXISTS" — a missing local half
    ...    is not an error; remote-only success is success.
    [Tags]    iop    iop-ext    5_6_6    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"id": $entity_id, "type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $endpoint, "operations": ["redirectionOps"]}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    ${r}=    Simple Vehicle Entity    ${entity_id}    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${r}

    ${response}=    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    204    ${response.status_code}
    ${gone}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    404    ${gone.status_code}

IOP_EXT_ERR_01_11 The 207 Body Is Exactly The 5.2.18 UpdateResult Shape
    [Documentation]    5.2.18: updated is a list of Attribute names,
    ...    notUpdated a list of NotUpdatedDetails; 5.2.19: attributeName +
    ...    reason (+ optional registrationId) — and nothing else.
    [Tags]    iop    iop-ext    5_2_18    5_2_19    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $dead_endpoint, "operations": ["redirectionOps"]}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    Create Local Entity
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Skoda"}}

    ${fragment}=    Evaluate
    ...    {"speed": {"type": "Property", "value": 9}, "brandName": {"type": "Property", "value": "VW"}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Check Response Status Code    207    ${response.status_code}
    ${body}=    Evaluate    $response.json()
    ${extra}=    Evaluate    [k for k in $body if k not in ("updated", "notUpdated", "@context")]
    Should Be Empty    ${extra}
    Should Contain    ${{ str($body['updated']) }}    brandName
    ${nu}=    Evaluate    $body['notUpdated'][0]
    Dictionary Should Contain Key    ${nu}    attributeName
    Dictionary Should Contain Key    ${nu}    reason
    ${nu_extra}=    Evaluate
    ...    [k for k in $nu if k not in ("attributeName", "reason", "registrationId")]
    Should Be Empty    ${nu_extra}

IOP_EXT_ERR_01_12 A Federated 404 Keeps Its Warnings
    [Documentation]    6.3.17: warnings indicate abnormal behaviour "even
    ...    when the retrieve ends 404" — the dead source's warning survives
    ...    on the ResourceNotFound response; the healthy 404 source adds
    ...    none.
    [Tags]    iop    iop-ext    6_3_17    5_7_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${dead}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $dead_endpoint}
    ${created}=    Post Registration At Broker    ${b1_url}    ${dead}
    Check Response Status Code    201    ${created.status_code}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b2_url}    ${etype}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    404    ${response.status_code}
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    ${parsed}=    Evaluate
    ...    __import__('re').findall(r'\\d{3} \\S+ "[^"]*"', $warning)
    Length Should Be    ${parsed}    1


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopErr${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopErr:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:ioperr-${suffix}
    Set Test Variable    ${server_started}    ${False}

Start Mock
    Start Server    ${mock_host}    ${mock_port}
    Set Test Variable    ${server_started}    ${True}

Register Mock As Source
    [Arguments]    ${id_scoped}=${False}    ${management}=${None}
    IF    ${id_scoped}
        ${info}=    Evaluate    [{"entities": [{"id": $entity_id, "type": $etype}]}]
    ELSE
        ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    END
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    IF    $management is not None
        Evaluate    $reg.update({"management": $management})
    END
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

Create Local Entity
    [Arguments]    ${entity_expr}
    ${entity}=    Evaluate    ${entity_expr}
    &{headers}=    Create Dictionary    Content-Type=application/json
    &{params}=    Create Dictionary    local=true
    ${response}=    POST    url=${b1_url}/entities    json=${entity}    params=${params}
    ...    headers=${headers}    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-2
    FOR    ${tail}    IN    ${EMPTY}    -l    -r
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
    END
    IF    ${server_started}
        Stop Server
    END
