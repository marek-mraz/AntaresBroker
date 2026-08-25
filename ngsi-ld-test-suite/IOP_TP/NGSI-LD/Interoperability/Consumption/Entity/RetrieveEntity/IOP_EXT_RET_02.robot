*** Settings ***
Documentation       Representation options on federated retrieval (Antares
...                 extension IOP TPs) — the single-broker representation
...                 TPs (keyValues, sysAttrs, attrs/pick/omit projection,
...                 lang, datasetId, GeoJSON, transient entities) replayed
...                 against data that lives in B2 and is retrieved via B1:
...                 4.5.x representations are applied by B1 AFTER the 4.5.5
...                 merge, so every one of them must behave identically for
...                 remote data.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}


*** Test Cases ***
IOP_EXT_RET_02_01 KeyValues Representation Of A Remote Entity
    [Documentation]    4.5.3 simplified representation applied by B1 to an
    ...    entity held in B2: speed collapses to its value.
    [Tags]    iop    iop-ext    4_5_3    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    options=keyValues
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']}    42

IOP_EXT_RET_02_02 SysAttrs Of A Remote Entity Survive The Merge
    [Documentation]    4.8/4.5.5.3: the forwarded read asks the peer for
    ...    sysAttrs (recency arbitration) — options=sysAttrs at B1 must
    ...    surface the remote createdAt/modifiedAt.
    [Tags]    iop    iop-ext    4_8    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    options=sysAttrs
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()['speed']}    createdAt
    Dictionary Should Contain Key    ${response.json()['speed']}    modifiedAt

IOP_EXT_RET_02_03 Attrs Projection Filters Remote Attributes
    [Documentation]    5.7.1.4 attrs parameter: only speed is requested —
    ...    B2's brandName must not appear in the merged entity.
    [Tags]    iop    iop-ext    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 42}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    attrs=speed
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    speed
    Dictionary Should Not Contain Key    ${response.json()}    brandName

IOP_EXT_RET_02_04 Pick Projection On A Remote Entity
    [Documentation]    4.5.24 pick: only the named members survive.
    [Tags]    iop    iop-ext    4_5_24    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 42}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    pick=id,type,speed
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    speed
    Dictionary Should Not Contain Key    ${response.json()}    brandName

IOP_EXT_RET_02_05 Omit Projection On A Remote Entity
    [Documentation]    4.5.24 omit: the named member is dropped, the rest
    ...    stays.
    [Tags]    iop    iop-ext    4_5_24    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 42}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    omit=brandName
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    speed
    Dictionary Should Not Contain Key    ${response.json()}    brandName

IOP_EXT_RET_02_06 Language Filter On A Remote LanguageProperty
    [Documentation]    4.5.18: lang=de against a languageMap stored in B2 —
    ...    the LanguageProperty collapses to the German string.
    [Tags]    iop    iop-ext    4_5_18    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "greeting": {"type": "LanguageProperty", "languageMap": {"en": "Hello", "de": "Hallo"}}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    lang=de
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    Hallo
    Should Not Contain    ${response.text}    Hello

IOP_EXT_RET_02_07 DatasetId Instances Split Across Brokers
    [Documentation]    4.5.5: the default instance lives in B1, a datasetId
    ...    instance in B2 — the merged Attribute carries BOTH instances.
    [Tags]    iop    iop-ext    4_5_5    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${local}=    Simple Vehicle Entity    ${entity_id}    ${etype}    10
    Create Entity At Broker    ${b1_url}    ${local}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 99, "datasetId": "urn:ngsi-ld:Dataset:gps"}}
    Create Entity At Broker    ${b2_url}    ${remote}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    ${speed}=    Get From Dictionary    ${response.json()}    speed
    ${instances}=    Evaluate    $speed if isinstance($speed, list) else [$speed]
    Length Should Be    ${instances}    2
    Should Contain    ${response.text}    urn:ngsi-ld:Dataset:gps

IOP_EXT_RET_02_08 GeoJSON Representation Of A Remote Entity
    [Documentation]    4.5.16/6.3.15: Accept application/geo+json via B1 —
    ...    the remote entity comes back as a Feature with its geometry.
    [Tags]    iop    iop-ext    4_5_16    6_3_15    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [8.6, 41.2]}}}
    Create Entity At Broker    ${b2_url}    ${entity}

    &{headers}=    Create Dictionary    Accept=application/geo+json
    ${eid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($entity_id, safe='')
    ${response}=    GET
    ...    url=${b1_url}/entities/${eid}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['type']}    Feature
    Should Contain    ${response.text}    Point

IOP_EXT_RET_02_09 Unknown Entity Stays 404 Despite The Registration
    [Documentation]    5.7.1.4/6.3.17: neither broker knows the id — the
    ...    federated retrieve is an honest 404, and a peer's 404 is NOT
    ...    abnormal behaviour (no NGSILD-Warning).
    [Tags]    iop    iop-ext    5_7_1    6_3_17    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning

IOP_EXT_RET_02_10 An Expired Remote Transient Entity Is Discarded
    [Documentation]    4.5.5.2/5.7.2.4: "any Entities where an expiresAt
    ...    DateTime is present and the date lies in the past shall be
    ...    discarded" — the transient entity in B2 expires and the
    ...    federated retrieve turns 404.
    [Tags]    iop    iop-ext    4_5_5_2    5_7_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${expires}=    Evaluate
    ...    (__import__('datetime').datetime.utcnow() + __import__('datetime').timedelta(seconds=2)).strftime('%Y-%m-%dT%H:%M:%SZ')
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "expiresAt": $expires, "speed": {"type": "Property", "value": 42}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}

    Sleep    3s
    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    404    ${response.status_code}


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
