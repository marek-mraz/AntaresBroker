*** Settings ***
Documentation       Two-broker TEMPORAL interoperability (Antares extension
...                 IOP TPs) — the case the official IOP suite never covers:
...                 the Temporal Evolution lives in B2 (or is split across
...                 B1 and B2) and is retrieved/queried via B1. Covers
...                 5.7.3.4 distributed temporal retrieval, 5.7.4.4
...                 distributed temporal query, the 4.11 window on remote
...                 instances and lastN over the merged series.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${TEMPORAL_OPS}=    ${{ ["retrieveTemporal", "queryTemporal"] }}


*** Test Cases ***
IOP_EXT_TMP_01_01 Remote Temporal Evolution Retrieved Via Broker1
    [Documentation]    5.7.3.4: the Temporal Evolution exists ONLY in B2; a
    ...    registration supporting retrieveTemporal makes it retrievable
    ...    through B1's /temporal/entities/{id}.
    [Tags]    iop    iop-ext    5_7_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "Property", "value": 20, "observedAt": "2026-05-02T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['id']}    ${entity_id}
    Should Contain    ${response.text}    2026-05-01T00:00:00Z
    Should Contain    ${response.text}    2026-05-02T00:00:00Z

IOP_EXT_TMP_01_02 Instances Merged Across Brokers
    [Documentation]    5.7.3.4: B1 holds one instance of the series, B2 two
    ...    more — retrieval via B1 returns the whole merged evolution.
    [Tags]    iop    iop-ext    5_7_3    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${local_doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${local_doc}
    ${remote_doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 20, "observedAt": "2026-05-02T00:00:00Z"}, {"type": "Property", "value": 30, "observedAt": "2026-05-03T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote_doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    2026-05-01T00:00:00Z
    Should Contain    ${response.text}    2026-05-02T00:00:00Z
    Should Contain    ${response.text}    2026-05-03T00:00:00Z

IOP_EXT_TMP_01_03 The Window Filters Remote Instances
    [Documentation]    4.11 between: only B2 instances inside [timeAt,
    ...    endTimeAt) may appear — the out-of-window instance must NOT.
    [Tags]    iop    iop-ext    5_7_3    4_11    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "Property", "value": 99, "observedAt": "2026-07-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=between
    ...    timeAt=2026-04-01T00:00:00Z
    ...    endTimeAt=2026-06-01T00:00:00Z
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    2026-05-01T00:00:00Z
    Should Not Contain    ${response.text}    2026-07-01T00:00:00Z

IOP_EXT_TMP_01_04 Temporal Query Federates To Broker2
    [Documentation]    5.7.4.4: the entity's evolution exists only in B2 —
    ...    a temporal QUERY (by type) through B1 includes it.
    [Tags]    iop    iop-ext    5_7_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Query Temporal Via Broker    ${b1_url}
    ...    type=${etype}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}
    Should Contain    ${response.text}    2026-05-01T00:00:00Z

IOP_EXT_TMP_01_05 LastN Applies To The Merged Series
    [Documentation]    5.7.4.4: "the last n timestamps (in descending
    ...    ordering) … within the concerned temporal interval" — over the
    ...    MERGED series: B1's old instance loses to B2's two newer ones.
    [Tags]    iop    iop-ext    5_7_3    5_7_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${local_doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${local_doc}
    ${remote_doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 20, "observedAt": "2026-05-02T00:00:00Z"}, {"type": "Property", "value": 30, "observedAt": "2026-05-03T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote_doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    lastN=2
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    2026-05-03T00:00:00Z
    Should Contain    ${response.text}    2026-05-02T00:00:00Z
    Should Not Contain    ${response.text}    2026-05-01T00:00:00Z


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}

Cleanup Interop Fixtures
    Delete Temporal Via Broker    ${b1_url}    ${entity_id}
    Delete Temporal Via Broker    ${b2_url}    ${entity_id}
    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    Delete Entity Via Broker    ${b2_url}    ${entity_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}
