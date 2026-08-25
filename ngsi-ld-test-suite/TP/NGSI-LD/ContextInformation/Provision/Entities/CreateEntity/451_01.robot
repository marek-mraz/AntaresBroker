*** Settings ***
Documentation       Verify the 4.5.1 Entity Representation prohibitions.
...
...                 Clause 4.5.1: "Terms defined in the Core Context as non-reified
...                 Properties (such as datasetId, instanceId, etc.) shall not be
...                 used as Attribute names." And: "Attributes shall not contain any
...                 embedded @context, as described in clause 5.5.7" — 5.5.7: such
...                 content "should result in an error of type BadRequestData".
...
...                 Antares extension TP — no official TP is tagged 4_5_1; both
...                 prohibitions are exactly the negative surface the official
...                 CreateEntity set skips.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Entity Expecting BadRequestData


*** Test Cases ***    ATTRIBUTES
451_01_01 Core Non-Reified Term datasetId As Attribute Name
    [Tags]    e-create    4_5_1    since_v1.9.1
    {"datasetId": {"type": "Property", "value": 1}}
451_01_02 Core Non-Reified Term instanceId As Attribute Name
    [Tags]    e-create    4_5_1    since_v1.9.1
    {"instanceId": {"type": "Property", "value": 1}}
451_01_03 Core Non-Reified Term observedAt As Attribute Name
    [Tags]    e-create    4_5_1    since_v1.9.1
    {"observedAt": {"type": "Property", "value": "2026-01-01T00:00:00Z"}}
451_01_04 Embedded Context Inside An Attribute
    [Tags]    e-create    4_5_1    5_5_7    since_v1.9.1
    {"speed": {"type": "Property", "value": 1, "@context": {"speed": "https://evil.example/speed"}}}


*** Keywords ***
Create Entity Expecting BadRequestData
    [Documentation]    4.5.1: an entity carrying the forbidden member shall be
    ...    rejected with BadRequestData; a control create without it succeeds.
    [Arguments]    ${attributes}
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${attrs}=    Evaluate    json.loads('''${attributes}''')    json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], **$attrs}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
