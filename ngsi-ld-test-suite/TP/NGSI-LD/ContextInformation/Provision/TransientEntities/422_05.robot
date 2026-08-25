*** Settings ***
Documentation       4.22 expiresAt set late and set to the past (Antares extension TP).
...                 An entity lives through several attribute updates (5.6.4 partial
...                 updates), THEN gets a future expiresAt via merge (legal at any
...                 time — 5.5.12 adds fragment members regardless of history) and is
...                 still served. Then expiresAt is set to an ALREADY-PAST DateTime:
...                 unlike Subscriptions (5.8.1 rejects past expiresAt with
...                 BadRequestData), no clause forbids a past value on an Entity —
...                 per 4.22 it "shall become invalid" at that instant, which has
...                 passed, so the entity answers 404 immediately, no waiting.

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Teardown      Delete Entity    ${entity_id}


*** Test Cases ***
422_05_01 ExpiresAt Set After Updates Then Set To Past Value
    [Tags]    transient    4_22
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "temperature": {"type": "Property", "value": 1}, "@context": $ngsild_test_suite_context}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    # Several attribute updates first (5.6.4 partial updates)
    Patch Temperature To    2
    Patch Temperature To    3
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Numbers    ${response.json()["temperature"]["value"]}    3    last partial update won
    # Now set a FUTURE expiration — the update history does not matter (5.5.12)
    ${now}=    Get Current Date    time_zone=UTC
    ${future}=    Add Time To Date    ${now}    30s    result_format=%Y-%m-%dT%H:%M:%SZ
    Merge ExpiresAt    ${future}
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    # Then set an ALREADY-PAST value — invalid at that instant, so 404 NOW
    Merge ExpiresAt    2020-01-01T00:00:00Z
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}


*** Keywords ***
Patch Temperature To
    [Arguments]    ${value}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    # int(): robot arguments are strings — raw $value would store a JSON string
    ${fragment}=    Evaluate
    ...    {"type": "Property", "value": int($value), "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/temperature
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}

Merge ExpiresAt
    [Arguments]    ${when}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${fragment}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "expiresAt": $when, "@context": $ngsild_test_suite_context}
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
