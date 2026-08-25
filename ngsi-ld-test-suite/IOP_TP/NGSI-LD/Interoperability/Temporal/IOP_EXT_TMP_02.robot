*** Settings ***
Documentation       Temporal representation and filter variants ported from
...                 the single-broker temporal TP tree to real two-broker
...                 setups (Antares extension IOP TPs): temporalValues and
...                 aggregatedValues rendered by B1 over remote series,
...                 attrs projection, the temporal q filter, before-windows,
...                 duplicate-instance dedup, idPattern, lastN+between and
...                 local=true scoping.

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
IOP_EXT_TMP_02_01 TemporalValues Format Over A Remote Series
    [Documentation]    4.5.9: options=temporalValues rendered by B1 for an
    ...    evolution held in B2 — values arrive as [value, timestamp] pairs.
    [Tags]    iop    iop-ext    4_5_9    5_7_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "Property", "value": 20, "observedAt": "2026-05-02T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    options=temporalValues
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()['speed']}    values
    Length Should Be    ${response.json()['speed']['values']}    2

IOP_EXT_TMP_02_02 Attrs Projection On A Remote Evolution
    [Documentation]    5.7.3.4 attrs: only the speed series is requested —
    ...    B2's rpm series must not appear.
    [Tags]    iop    iop-ext    5_7_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}], "rpm": [{"type": "Property", "value": 3000, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    attrs=speed
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    speed
    Dictionary Should Not Contain Key    ${response.json()}    rpm

IOP_EXT_TMP_02_03 Temporal Q Filter Selects By Remote Values
    [Documentation]    5.7.4.4 S2: q=speed>50 — only the entity whose REMOTE
    ...    instances satisfy the filter within the window is returned.
    [Tags]    iop    iop-ext    5_7_4    4_9    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${fast}=    Evaluate
    ...    {"id": $entity_id + "-fast", "type": $etype, "speed": [{"type": "Property", "value": 99, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${fast}
    ${slow}=    Evaluate
    ...    {"id": $entity_id + "-slow", "type": $etype, "speed": [{"type": "Property", "value": 5, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${slow}

    ${response}=    Query Temporal Via Broker    ${b1_url}
    ...    type=${etype}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    q=speed>50
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-fast
    Should Not Contain    ${response.text}    ${entity_id}-slow

IOP_EXT_TMP_02_04 Before-Window Excludes Later Remote Instances
    [Documentation]    4.11 before: instances at/after timeAt must not
    ...    appear — checked against a remote series.
    [Tags]    iop    iop-ext    4_11    5_7_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "Property", "value": 20, "observedAt": "2026-06-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=before
    ...    timeAt=2026-05-15T00:00:00Z
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    2026-05-01T00:00:00Z
    Should Not Contain    ${response.text}    2026-06-01T00:00:00Z

IOP_EXT_TMP_02_05 The Same Instance In Both Brokers Appears Once
    [Documentation]    5.7.3.4: merged instance sets deduplicate by the
    ...    timeproperty value — an instance present in B1 AND B2 with the
    ...    same observedAt is served exactly once.
    [Tags]    iop    iop-ext    5_7_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${doc}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    options=temporalValues
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()['speed']['values']}    1

IOP_EXT_TMP_02_06 IdPattern On A Federated Temporal Query
    [Documentation]    5.7.4.4: idPattern filters the federated temporal
    ...    result set.
    [Tags]    iop    iop-ext    5_7_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${wanted}=    Evaluate
    ...    {"id": $entity_id + "-wanted", "type": $etype, "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${wanted}
    ${other}=    Evaluate
    ...    {"id": $entity_id + "-other", "type": $etype, "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${other}

    ${response}=    Query Temporal Via Broker    ${b1_url}
    ...    type=${etype}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    idPattern=.*wanted$
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-wanted
    Should Not Contain    ${response.text}    ${entity_id}-other

IOP_EXT_TMP_02_07 LastN Within A Between-Window Over Merged Series
    [Documentation]    5.7.4.4: lastN counts within the temporal interval —
    ...    the window first excludes the newest instance, then lastN=1
    ...    keeps only the newest REMAINING one across both brokers.
    [Tags]    iop    iop-ext    5_7_3    4_11    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${local_doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${local_doc}
    ${remote_doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 20, "observedAt": "2026-05-02T00:00:00Z"}, {"type": "Property", "value": 30, "observedAt": "2026-07-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote_doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=between
    ...    timeAt=2026-04-01T00:00:00Z
    ...    endTimeAt=2026-06-01T00:00:00Z
    ...    lastN=1
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    2026-05-02T00:00:00Z
    Should Not Contain    ${response.text}    2026-05-01T00:00:00Z
    Should Not Contain    ${response.text}    2026-07-01T00:00:00Z

IOP_EXT_TMP_02_08 SysAttrs On A Remote Temporal Evolution
    [Documentation]    4.8: options=sysAttrs on the federated temporal
    ...    retrieve surfaces the remote instances' createdAt.
    [Tags]    iop    iop-ext    4_8    5_7_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    options=sysAttrs
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    createdAt

IOP_EXT_TMP_02_09 local=true Excludes The Remote Evolution
    [Documentation]    5.5.13: with local scope the temporal query must not
    ...    reach B2 — B2's evolution is invisible.
    [Tags]    iop    iop-ext    5_5_13    5_7_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote_doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote_doc}

    ${response}=    Query Temporal Via Broker    ${b1_url}
    ...    type=${etype}
    ...    timerel=after
    ...    timeAt=2020-01-01T00:00:00Z
    ...    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${entity_id}

IOP_EXT_TMP_02_10 Aggregated Values Over A Remote Series
    [Documentation]    4.5.19: options=aggregatedValues with aggrMethods=avg
    ...    computed by B1 over the series held in B2.
    [Tags]    iop    iop-ext    4_5_19    5_7_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${doc}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 10, "observedAt": "2026-05-01T00:00:00Z"}, {"type": "Property", "value": 30, "observedAt": "2026-05-01T12:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${doc}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=between
    ...    timeAt=2026-05-01T00:00:00Z
    ...    endTimeAt=2026-05-02T00:00:00Z
    ...    options=aggregatedValues
    ...    aggrMethods=avg
    ...    aggrPeriodDuration=P1D
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()['speed']}    avg
    Should Contain    ${response.text}    20


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}

Cleanup Interop Fixtures
    FOR    ${tail}    IN    ${EMPTY}    -fast    -slow    -wanted    -other
        Delete Temporal Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Temporal Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
    END
    Delete Registration At Broker    ${b1_url}    ${registration_id}
