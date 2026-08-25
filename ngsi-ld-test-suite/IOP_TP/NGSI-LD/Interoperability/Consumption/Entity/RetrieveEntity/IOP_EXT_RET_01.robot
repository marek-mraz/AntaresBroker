*** Settings ***
Documentation       Two-broker retrieval edges (Antares extension IOP TPs).
...
...                 B1 federates to B2 through a Context Source Registration;
...                 the data lives in B2 (or is split across both) and every
...                 request goes through B1. Covers 5.7.1.4 distributed
...                 retrieval, the 4.5.5 merge algorithm (split entities,
...                 4.5.5.3 recency conflict resolution), 4.3.6.2 auxiliary
...                 semantics and 4.3.6.1 registration-scope narrowing —
...                 none of which the official IOP_CNF TPs exercise beyond
...                 the basic inclusive/exclusive retrieve.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}


*** Test Cases ***
IOP_EXT_RET_01_01 Remote-Only Entity Retrieved Via Broker1
    [Documentation]    5.7.1.4: the entity exists ONLY in B2; an inclusive
    ...    registration at B1 makes it retrievable through B1. local=true at
    ...    B1 must still 404 — the data was never copied.
    [Tags]    iop    iop-ext    5_7_1    4_3_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${entity_id}
    Should Contain    ${response.text}    speed

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_RET_01_02 Split Entity Merged Across Brokers
    [Documentation]    4.5.5: half the entity (brandName) is local to B1,
    ...    the other half (speed) lives in B2 — retrieval via B1 returns ONE
    ...    entity carrying both Attributes.
    [Tags]    iop    iop-ext    4_5_5    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local_half}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local_half}
    ${remote_half}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${remote_half}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    brandName
    Dictionary Should Contain Key    ${response.json()}    speed

IOP_EXT_RET_01_03 Newer Remote Instance Wins The Conflict
    [Documentation]    4.5.5.3: both brokers hold the same Attribute; the
    ...    instance with the most recent observedAt (B2's) is served.
    [Tags]    iop    iop-ext    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Simple Vehicle Entity    ${entity_id}    ${etype}    10    2026-01-01T00:00:00Z
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    99    2026-02-01T00:00:00Z
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    99

IOP_EXT_RET_01_04 Newer Local Instance Wins The Conflict
    [Documentation]    4.5.5.3 mirror case: B1's local instance is the most
    ...    recent one — the remote value must NOT override it.
    [Tags]    iop    iop-ext    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Simple Vehicle Entity    ${entity_id}    ${etype}    10    2026-03-01T00:00:00Z
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    99    2026-02-01T00:00:00Z
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    10

IOP_EXT_RET_01_05 Auxiliary Data Never Overrides Local Data
    [Documentation]    4.3.6.2: "An auxiliary Context Source Registration
    ...    never overrides data held directly within a Context Broker" —
    ...    even when B2's instance is NEWER, B1's local value stays; the
    ...    auxiliary data only fills the Attribute B1 lacks.
    [Tags]    iop    iop-ext    4_3_6_2    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=auxiliary
    ${local}=    Simple Vehicle Entity    ${entity_id}    ${etype}    10    2026-01-01T00:00:00Z
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 99, "observedAt": "2026-02-01T00:00:00Z"}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    10
    Dictionary Should Contain Key    ${response.json()}    brandName

IOP_EXT_RET_01_06 Registration Scope Narrows What Broker1 Pulls
    [Documentation]    4.3.6.1: "all constraints specified in the
    ...    registration shall be respected" — the registration offers only
    ...    the speed Property, so B2's brandName must never surface via B1.
    [Tags]    iop    iop-ext    4_3_6_1    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"type": $etype}], "propertyNames": ["speed"]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 42}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    speed
    Dictionary Should Not Contain Key    ${response.json()}    brandName


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}

Cleanup Interop Fixtures
    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    Delete Entity Via Broker    ${b2_url}    ${entity_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}
