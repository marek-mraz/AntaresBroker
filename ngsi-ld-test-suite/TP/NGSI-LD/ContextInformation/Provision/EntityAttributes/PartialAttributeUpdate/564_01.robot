*** Settings ***
Documentation       Verify 5.6.4 Partial Attribute Update edge behaviour.
...
...                 5.6.4.4: "If the target Attribute is scope, then an error
...                 of type BadRequestData shall be raised"; the optional
...                 ?type selector (4.17) narrows the target entity —
...                 mismatch means ResourceNotFound.
...
...                 Antares extension TP — official 007_x TPs cover neither.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
564_01_01 Scope Cannot Be Partially Updated
    [Documentation]    5.6.4.4: PATCH attrs/scope → 400 BadRequestData; the
    ...    stored scope stays untouched.
    [Tags]    ea-partial-update    5_6_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "scope": "/Madrid", "name": {"type": "Property", "value": "x"}}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${fragment}=    Evaluate    {"value": "/Barcelona"}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/scope
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}
    Should Be Equal    ${response.json()['scope']}    /Madrid
    [Teardown]    Delete Entity    ${entity_id}

564_01_02 Type Selector Gates The Partial Update
    [Documentation]    5.6.4.4: ?type not matching the target → 404; matching
    ...    → 204 with the sub-attribute overwritten.
    [Tags]    ea-partial-update    5_6_4    4_17    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "name": {"type": "Property", "value": "x"}}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${fragment}=    Evaluate    {"value": "y"}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/name?type=Vehicle
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/name?type=Building
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Should Be Equal    ${response.json()['name']['value']}    y
    [Teardown]    Delete Entity    ${entity_id}
