*** Settings ***
Documentation       Verify 5.6.2 Update Entity Attributes type selector.
...
...                 5.6.2.3/5.6.2.4: the operation takes an optional selector
...                 of Entity types (4.17); if there is no entity with the
...                 target id AND the specified type, ResourceNotFound shall
...                 be raised.
...
...                 Antares extension TP — official 011_x TPs never send the
...                 type selector.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
562_01_01 Type Selector Gates The Update
    [Documentation]    5.6.2.4: ?type not matching the target → 404 and no
    ...    write; matching type → 204 and the attribute is updated.
    [Tags]    ea-update    5_6_2    4_17    since_v1.9.1
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
    ${fragment}=    Evaluate    {"name": {"type": "Property", "value": "y"}}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/?type=Vehicle
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Should Be Equal    ${response.json()['name']['value']}    x
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/?type=Building
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Should Be Equal    ${response.json()['name']['value']}    y
    [Teardown]    Delete Entity    ${entity_id}
