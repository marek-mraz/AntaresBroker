*** Settings ***
Documentation       ADR-001 URN grammar edge cases on the wire (Antares
...                 extension IOP TPs). 5.2.8: idPattern is a "Regular
...                 expression as per IEEE 1003.2" — an unescaped dot
...                 matches ANY character (why ADR-001 razidlos use `_`
...                 and validator patterns escape metacharacters); URIs
...                 are opaque to the broker, so hierarchy colons and
...                 Crockford-base32 suffixes in the local segment route
...                 exactly like natural keys (5.12). 4.5.2: an Entity
...                 may have multiple Entity Types; 5.12 matches when the
...                 type selector matches ANY of the EntityInfo types.
...                 4.14 multi-tenancy + 5.2.9 tenant: "the Tenant to
...                 specify in all requests to this Context Source" — the
...                 client tenant never propagates; 4.20 Table 4.20-1:
...                 federationOps contains NO provision operations, so a
...                 write through such a CSR must not reach the source.

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
IOP_EXT_IDR_06_46 Hierarchy Colons In The Local Segment Route Through The Prefix
    [Documentation]    5.12: the URN's local segment carries extra
    ...    hierarchy colons and a hyphenated token
    ...    (…odpady:kontajner:sektor-a:0042) — the anchored razidlo prefix
    ...    pattern still routes the retrieve; URIs are opaque beyond the
    ...    regex.
    [Tags]    iop    iop-ext    5_12    5_2_8    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_hier}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_hier}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_hier}

IOP_EXT_IDR_06_47 A Crockford-Base32 Suffix Routes Like A Natural Key
    [Documentation]    5.12: ADR-001's random-suffix form (Crockford
    ...    base32, no vowels/ambiguous chars) routes through the same
    ...    anchored prefix exactly like a natural key.
    [Tags]    iop    iop-ext    5_12    5_2_8    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_b32}    ${0.5}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_b32}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${eid_b32}

IOP_EXT_IDR_06_48 An Unescaped Dot Over-Matches; The Escaped Dot Does Not
    [Documentation]    5.2.8 (IEEE 1003.2): in the pattern
    ...    "sk.banskabystrica" the unescaped dot matches ANY character —
    ...    it routes a sk_banskabystrica id (over-match documented: why
    ...    ADR-001 validator patterns escape metacharacters). The escaped
    ...    form "sk\\.banskabystrica" must NOT match the underscore id:
    ...    the source records ZERO requests.
    [Tags]    iop    iop-ext    5_2_8    5_12    since_v1.9.1
    ${pat_dot}=    Evaluate    "^" + $base + ":sk.banskabystrica:odpady:.*$"
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_dot}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Delete Registration At Broker    ${b1_url}    ${registration_id}

    Start Mock
    ${pat_esc}=    Evaluate    "^" + $base + ":sk\\\\.banskabystrica:odpady:.*$"
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_esc}]}]
    Register Mock As Idr Source    ${info}    ${registration_id}-2
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_presov}
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Should Be True    ${response.status_code} in (200, 404)
    Wait For No Request    ${2}

IOP_EXT_IDR_06_49 A Multi-Type Entity Still Routes Via The Registered Supertype
    [Documentation]    4.5.2 + 5.12: the entity carries TWO Entity Types;
    ...    the CSR registers the supertype with the razidlo pattern — the
    ...    retrieve via B1 works and the response carries both types.
    [Tags]    iop    iop-ext    4_5_2    5_12    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Evaluate
    ...    {"id": $eid_bb, "type": [$etype, $etype + "Camera"], "fillLevel": {"type": "Property", "value": 0.42}}
    Create Entity At Broker    ${b2_url}    ${e}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${etype}Camera
    Should Contain    ${response.text}    fillLevel

IOP_EXT_IDR_06_50 The Registration Tenant Rules The Forward; federationOps Blocks Writes
    [Documentation]    4.14 + 5.2.9 tenant: the forward to the source uses
    ...    the REGISTRATION's tenant — the entity living only in B2's
    ...    "mesto" tenant resolves via B1's default tenant, and B2's
    ...    default tenant never holds it. 4.20: the CSR's
    ...    operations:["federationOps"] contains no provision operations,
    ...    so an attribute write through it must NOT reach B2 — the mesto
    ...    value stays unchanged (both halves of the ADR-001 read-only
    ...    agent-tenant posture).
    [Tags]    iop    iop-ext    4_14    4_20    5_12    since_v1.9.1
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "operations": ["federationOps"], "information": $info, "endpoint": $endpoint, "tenant": "mesto"}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}    mesto

    ${response}=    Get Entity Via Broker    ${b1_url}    ${eid_bb}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    fillLevel
    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    local=true
    Check Response Status Code    404    ${response.status_code}

    ${patch}=    Evaluate    {"fillLevel": {"type": "Property", "value": 0.99}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${eid_bb}    ${patch}
    Should Be True    ${response.status_code} in (207, 404, 409)
    ${response}=    Get Entity Via Broker    ${b2_url}    ${eid_bb}    mesto    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Numbers    ${response.json()['fillLevel']['value']}    0.42


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    WasteC${suffix}
    Set Test Variable    ${base}    urn:ngsi-ld:WasteC${suffix}
    ${pat_bb}=    Evaluate    "^" + $base + ":sk_banskabystrica:odpady:.*$"
    Set Test Variable    ${pat_bb}
    Set Test Variable    ${eid_bb}    ${base}:sk_banskabystrica:odpady:kontajner:0042
    Set Test Variable    ${eid_hier}    ${base}:sk_banskabystrica:odpady:kontajner:sektor-a:0042
    Set Test Variable    ${eid_b32}    ${base}:sk_banskabystrica:odpady:01J8ZX7M2E9V3QK5T8YWDF4RGB
    Set Test Variable    ${eid_presov}    ${base}:sk_presov:odpady:kontajner:0001
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
    FOR    ${eid}    IN    ${eid_bb}    ${eid_hier}    ${eid_b32}    ${eid_presov}
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
    END
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($eid_bb, safe='')
    &{headers}=    Create Dictionary    NGSILD-Tenant=mesto
    ${response}=    DELETE    url=${b2_url}/entities/${eid}    headers=${headers}    expected_status=any
    IF    ${server_started}
        Stop Server
    END
