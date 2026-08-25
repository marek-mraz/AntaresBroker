*** Settings ***
Documentation       Verify 4.6.6 Ordering of Entities in arrays having more than
...                 one instance of the same Entity.
...
...                 4.6.6: duplicate instances "shall come in chronological order,
...                 i.e. the first entity instance in the array shall be older
...                 than the second" — the broker applies them sequentially, so
...                 the LAST occurrence determines the final state.
...
...                 Antares extension TP — official duplicate TPs (003_10, 004_07,
...                 006_04) assert only the status code, never the resulting
...                 entity state / ordering semantics.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
466_01_01 Later Duplicate Instance Determines Final State
    [Documentation]    4.6.6: upsert batch with the same id twice — the second
    ...    (newer) instance's value must win and the first instance's
    ...    first-only attribute must NOT survive (replace semantics).
    [Tags]    be-upsert    4_6_6    5_5_11_0    5_5_11_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${first}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 1}, "old": {"type": "Property", "value": True}}
    ${second}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 2}}
    ${response}=    Batch Upsert Entities    ${first}    ${second}
    Should Be True    $response.status_code in (201, 204)
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${speed}=    Evaluate    $response.json()['speed']['value']
    Should Be Equal As Integers    ${speed}    2
    ${has_old}=    Evaluate    'old' in $response.json()
    Should Not Be True    ${has_old}    first instance's attrs must not linger
    [Teardown]    Delete Entity    ${entity_id}
