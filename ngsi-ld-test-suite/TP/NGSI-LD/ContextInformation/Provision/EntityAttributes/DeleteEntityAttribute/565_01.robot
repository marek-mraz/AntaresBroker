*** Settings ***
Documentation       Verify 5.6.5 Delete Attribute edge behaviour.
...
...                 5.6.5.4: the ?type selector narrows the target entity
...                 (mismatch → ResourceNotFound, nothing deleted); "If the
...                 target Attribute is scope, remove the scope Attribute".
...
...                 Antares extension TP — official 008_x TPs cover neither.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
565_01_01 Type Selector Gates The Delete And Scope Is Deletable
    [Documentation]    5.6.5.4: DELETE attrs/name?type=Vehicle on a Building →
    ...    404 with the attribute intact; matching selector deletes; DELETE
    ...    attrs/scope removes the scope member.
    [Tags]    ea-delete    5_6_5    4_17    since_v1.9.1
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
    ${response}=    DELETE
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/name?type=Vehicle
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Should Be Equal    ${response.json()['name']['value']}    x
    ${response}=    DELETE
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/name?type=Building
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    # scope is deletable via Delete Attribute
    ${response}=    DELETE
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/scope
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.json()}    scope
    Dictionary Should Not Contain Key    ${response.json()}    name
    [Teardown]    Delete Entity    ${entity_id}
