*** Settings ***
Documentation       Verify 5.6.7.4 batch input validation.
...
...                 5.6.7.4: "If the input Array is empty or contains a null
...                 value in any of its items an error of type BadRequestData
...                 shall be raised" — the whole request fails atomically.
...
...                 Antares extension TP — official 003_x TPs never send
...                 null items.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
567_01_01 Null Item Fails The Whole Batch
    [Documentation]    5.6.7.4: [entity, null] → 400 BadRequestData and the
    ...    valid sibling is NOT created; [] → 400 too.
    [Tags]    be-create    5_6_7    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${batch}=    Evaluate    [{"id": $entity_id, "type": "Vehicle"}, None]
    ${response}=    POST
    ...    url=${url}/entityOperations/create
    ...    json=${batch}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    404    ${response.status_code}
    ${empty}=    Evaluate    []
    ${response}=    POST
    ...    url=${url}/entityOperations/create
    ...    json=${empty}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
