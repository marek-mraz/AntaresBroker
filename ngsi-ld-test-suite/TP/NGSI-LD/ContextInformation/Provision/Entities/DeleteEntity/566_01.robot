*** Settings ***
Documentation       Verify 5.6.6 Delete Entity type selector.
...
...                 5.6.6.4: "there is no existing Entity whose id (URI), and
...                 where specified type, is equivalent held locally" →
...                 ResourceNotFound; the entity must survive a delete gated
...                 by a non-matching ?type selector.
...
...                 Antares extension TP — official 013_x TPs never send the
...                 type selector.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
566_01_01 Type Selector Gates The Entity Delete
    [Documentation]    5.6.6.4: DELETE ?type=Vehicle on a Building → 404 and
    ...    the entity survives; ?type=Building deletes it.
    [Tags]    e-delete    5_6_6    4_17    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${payload}=    Evaluate    {"id": $entity_id, "type": "Building"}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${response}=    DELETE
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}?type=Vehicle
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    ${response}=    DELETE
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}?type=Building
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    404    ${response.status_code}
