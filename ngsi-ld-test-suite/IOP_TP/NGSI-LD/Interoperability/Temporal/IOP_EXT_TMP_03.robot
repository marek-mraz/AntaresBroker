*** Settings ***
Documentation       Distributed temporal edges (Antares extension IOP TPs).
...                 5.7.3.4: remote Attribute data "is then merged together
...                 with S1 according to the algorithm defined in clause
...                 4.5.5"; a source answering 404 "should not be considered
...                 as abnormal behaviour" (6.3.17). 5.7.4.4: lastN "refers
...                 to a number, n, of Attribute instances which shall
...                 correspond to the last n timestamps (in descending
...                 ordering) ... within the concerned temporal interval";
...                 aggregation per 4.5.19 applies to the merged result;
...                 pagination per 5.5.9. 5.2.9 observationInterval: "A
...                 temporal query based on the observedAt Temporal
...                 Property ... is matched against the observationInterval
...                 for overlap". 6.24: POST /temporal/entityOperations/
...                 query binds 5.7.4 like the GET form.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}
${TEMPORAL_OPS}=    ${{ ["retrieveTemporal", "queryTemporal"] }}


*** Test Cases ***
IOP_EXT_TMP_03_01 Aggregation Runs Over The Merged Series
    [Documentation]    5.7.4.4 + 4.5.19: aggregated values are computed on
    ...    the result AFTER the remote series merged into S4 — avg over one
    ...    local (100) and one remote (300) instance is 200, never either
    ...    per-broker value.
    [Tags]    iop    iop-ext    5_7_4    4_5_19    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 100, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${local}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 300, "observedAt": "2026-05-02T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${response}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=between    timeAt=2026-04-30T00:00:00Z    endTimeAt=2026-05-03T00:00:00Z
    ...    format=aggregatedValues    aggrMethods=avg
    Check Response Status Code    200    ${response.status_code}
    ${avg}=    Evaluate    $response.json()[0]['speed']['avg'][0][0]
    Should Be Equal As Numbers    ${avg}    200
    Should Not Be Equal As Numbers    ${avg}    100

IOP_EXT_TMP_03_02 scopeQ Filters The Federated Temporal Result
    [Documentation]    5.7.4.4 S4/4.18: "If the Scope query is present ...
    ...    select those Entities whose Entity Scope instances match the
    ...    Scope query" — only the remote entity carrying the scope
    ...    survives.
    [Tags]    iop    iop-ext    5_7_4    4_18    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${in}=    Evaluate
    ...    {"id": $entity_id + "-in", "type": $etype, "scope": "/Madrid", "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${in}
    ${out}=    Evaluate
    ...    {"id": $entity_id + "-out", "type": $etype, "scope": "/Paris", "speed": [{"type": "Property", "value": 2, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${out}

    ${response}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z    scopeQ=/Madrid
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-in
    Should Not Contain    ${response.text}    ${entity_id}-out

IOP_EXT_TMP_03_03 timeproperty=modifiedAt Windows Remote Instances
    [Documentation]    5.7.4.4/5.2.21: the temporal query's timeproperty
    ...    selects which Temporal Property the window applies to — a
    ...    modifiedAt window in the past matches freshly stored remote
    ...    instances, a future one matches nothing.
    [Tags]    iop    iop-ext    5_7_4    5_2_21    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 5, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${past}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z    timeproperty=modifiedAt
    Check Response Status Code    200    ${past.status_code}
    Should Contain    ${past.text}    ${entity_id}
    ${future}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=2100-01-01T00:00:00Z    timeproperty=modifiedAt
    Check Response Status Code    200    ${future.status_code}
    Should Not Contain    ${future.text}    ${entity_id}

IOP_EXT_TMP_03_04 Temporal Pages Partition The Federated Union
    [Documentation]    5.7.4.4 + 5.5.9: pagination over the merged temporal
    ...    result — three entities across two brokers, pages of two
    ...    partition them without repeats.
    [Tags]    iop    iop-ext    5_7_4    5_5_9    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    FOR    ${broker}    ${tail}    IN    ${b1_url}    -a    ${b2_url}    -b    ${b2_url}    -c
        ${doc}=    Evaluate
        ...    {"id": $entity_id + $tail, "type": $etype, "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]}
        Upsert Temporal At Broker    ${broker}    ${doc}
    END

    ${page1}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z    limit=2
    Check Response Status Code    200    ${page1.status_code}
    Length Should Be    ${page1.json()}    2
    ${page2}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z    limit=2    offset=2
    Check Response Status Code    200    ${page2.status_code}
    Length Should Be    ${page2.json()}    1
    ${union}=    Evaluate    {e["id"] for e in $page1.json()} | {e["id"] for e in $page2.json()}
    Length Should Be    ${union}    3

IOP_EXT_TMP_03_05 A Remote Attribute Deletion Appears As A Deletion Instance
    [Documentation]    4.5.7 + 5.7.3.4: attribute deletion is recorded in
    ...    the Temporal Evolution with the value urn:ngsi-ld:null and a
    ...    deletedAt — merged from the remote broker like any instance. Per
    ...    4.11 the deletion instance matches the deletedAt timeproperty
    ...    window (the tombstone carries no created/observed timestamps).
    [Tags]    iop    iop-ext    5_7_3    4_5_7    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    Create Entity At Broker    ${b2_url}    ${e}
    Delete Entity Attr Via Broker    ${b2_url}    ${entity_id}    speed

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z    timeproperty=deletedAt
    ...    options=sysAttrs
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    urn:ngsi-ld:null
    Should Contain    ${response.text}    deletedAt
    Should Not Contain    ${response.text}    "value":5

IOP_EXT_TMP_03_06 A Remote Temporal 404 Serves The Local Series Silently
    [Documentation]    5.7.3.4 with 6.3.17: "a registration endpoint
    ...    responding with no data and the HTTP status code 404 - Not Found
    ...    should not be considered as abnormal behaviour" — the local
    ...    series is served with no NGSILD-Warning.
    [Tags]    iop    iop-ext    5_7_3    6_3_17    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 9, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${local}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    "value":9
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning

IOP_EXT_TMP_03_07 observationInterval Gates The Temporal Forward
    [Documentation]    5.2.9: "The observationInterval specifies the time
    ...    interval for which the Context Source can provide Entity
    ...    information ... A temporal query based on the observedAt
    ...    Temporal Property ... is matched against the observationInterval
    ...    for overlap" — a query window outside the interval is never
    ...    forwarded, so even remote instances that would match stay
    ...    invisible.
    [Tags]    iop    iop-ext    5_2_9    5_9_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint, "operations": ["retrieveTemporal", "queryTemporal"], "observationInterval": {"startAt": "2026-01-01T00:00:00Z", "endAt": "2026-02-01T00:00:00Z"}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 15, "observedAt": "2026-01-15T00:00:00Z"}, {"type": "Property", "value": 35, "observedAt": "2026-03-15T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${march}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=between    timeAt=2026-03-01T00:00:00Z    endTimeAt=2026-04-01T00:00:00Z
    Check Response Status Code    200    ${march.status_code}
    Should Not Contain    ${march.text}    ${entity_id}
    ${january}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=between    timeAt=2026-01-01T00:00:00Z    endTimeAt=2026-02-01T00:00:00Z
    Check Response Status Code    200    ${january.status_code}
    Should Contain    ${january.text}    ${entity_id}
    Should Contain    ${january.text}    "value":15

IOP_EXT_TMP_03_08 POST Temporal Query Federates Like GET
    [Documentation]    6.24 binding of 5.7.4: POST
    ...    /temporal/entityOperations/query with a Query document returns
    ...    the same federated result as the GET form.
    [Tags]    iop    iop-ext    5_7_4    6_24    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 27, "observedAt": "2026-05-01T00:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${get}=    Query Temporal Via Broker    ${b1_url}    type=${etype}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z
    Check Response Status Code    200    ${get.status_code}
    Should Contain    ${get.text}    ${entity_id}
    ${query}=    Evaluate
    ...    {"type": "Query", "entities": [{"type": $etype}], "temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${post}=    POST    url=${b1_url}/temporal/entityOperations/query    json=${query}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    200    ${post.status_code}
    Should Contain    ${post.text}    ${entity_id}
    Should Contain    ${post.text}    "value":27
    Should Not Contain    ${post.text}    ${entity_id}-x

IOP_EXT_TMP_03_09 lastN Caps The Merged Series, Not The Per-Broker Ones
    [Documentation]    5.7.4.4: lastN selects "the last n timestamps (in
    ...    descending ordering) ... within the concerned temporal interval"
    ...    — applied to the MERGED series: the newest local instance plus
    ...    the two newest remote ones survive, older remote instances drop.
    [Tags]    iop    iop-ext    5_7_4    5_2_21    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 111, "observedAt": "2026-05-01T01:00:00Z"}, {"type": "Property", "value": 222, "observedAt": "2026-05-01T02:00:00Z"}, {"type": "Property", "value": 333, "observedAt": "2026-05-01T03:00:00Z"}, {"type": "Property", "value": 444, "observedAt": "2026-05-01T04:00:00Z"}, {"type": "Property", "value": 555, "observedAt": "2026-05-01T05:00:00Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 666, "observedAt": "2026-05-01T10:00:00Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${local}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z    lastN=3
    Check Response Status Code    200    ${response.status_code}
    ${instances}=    Evaluate
    ...    $response.json()['speed'] if isinstance($response.json()['speed'], list) else [$response.json()['speed']]
    Length Should Be    ${instances}    3
    Should Contain    ${response.text}    "value":666
    Should Contain    ${response.text}    "value":555
    Should Not Contain    ${response.text}    "value":111
    Should Not Contain    ${response.text}    "value":222

IOP_EXT_TMP_03_10 Same datasetId And Timestamp Across Brokers Merge To One Instance
    [Documentation]    5.7.3.4 merge per 4.5.5: instances sharing datasetId
    ...    and the same timeproperty slot collapse — exactly one instance
    ...    survives for the slot, never both values.
    [Tags]    iop    iop-ext    5_7_3    4_5_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${TEMPORAL_OPS}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 700, "datasetId": "urn:ngsi-ld:ds:x", "observedAt": "2026-05-05T05:05:05Z"}]}
    Upsert Temporal At Broker    ${b1_url}    ${local}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": [{"type": "Property", "value": 800, "datasetId": "urn:ngsi-ld:ds:x", "observedAt": "2026-05-05T05:05:05Z"}]}
    Upsert Temporal At Broker    ${b2_url}    ${remote}

    ${response}=    Get Temporal Via Broker    ${b1_url}    ${entity_id}
    ...    timerel=after    timeAt=2020-01-01T00:00:00Z
    Check Response Status Code    200    ${response.status_code}
    ${slots}=    Evaluate    $response.text.count('2026-05-05T05:05:05')
    Should Be Equal As Integers    ${slots}    1
    ${both}=    Evaluate    ('"value":700' in $response.text) and ('"value":800' in $response.text)
    Should Not Be True    ${both}


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopTmc${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopTmc:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:ioptmc-${suffix}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    FOR    ${tail}    IN    ${EMPTY}    -a    -b    -c    -in    -out
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Temporal Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Temporal Via Broker    ${b2_url}    ${entity_id}${tail}
    END
