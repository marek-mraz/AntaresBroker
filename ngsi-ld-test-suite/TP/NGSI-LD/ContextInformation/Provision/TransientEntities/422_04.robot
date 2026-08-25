*** Settings ***
Documentation       4.22 expiration set AFTER creation (Antares extension TP).
...                 Create a durable entity, then set expiresAt = now+3s via a
...                 Merge Entity patch (5.6.17 / 5.5.12 — fragment members not in
...                 the target are added), confirm it is still served while alive,
...                 wait 5 s, confirm it is gone.

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Teardown      Delete Entity    ${entity_id}


*** Test Cases ***
422_04_01 Expiration Set By Update Takes Effect
    [Tags]    transient    4_22
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "name": {"type": "Property", "value": "durable-at-birth"}, "@context": $ngsild_test_suite_context}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    # Set the expiration afterwards (merge patch adds expiresAt)
    ${now}=    Get Current Date    time_zone=UTC
    ${expiry}=    Add Time To Date    ${now}    3s    result_format=%Y-%m-%dT%H:%M:%SZ
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${fragment}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "expiresAt": $expiry, "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    # Still served while alive
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Sleep    5s
    # Gone once the expiration set by the update has passed
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
