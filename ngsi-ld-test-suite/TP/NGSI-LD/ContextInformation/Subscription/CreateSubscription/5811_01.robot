*** Settings ***
Documentation       Verify 5.8.1.4 Create Subscription edges the official
...                 028 TPs skip.
...
...                 5.8.1.4: "If the expiration timestamp provided
...                 represents a moment before the current date and time,
...                 then an error of type BadRequestData shall be raised";
...                 "If the value of the isActive field is false, then the
...                 initial status of the Subscription shall be set to
...                 paused"; "If the subscription document does not include
...                 a Subscription identifier, a new locally unique
...                 identifier (URI) shall be automatically generated".
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
5811_01_01 Past ExpiresAt Is BadRequestData
    [Documentation]    5.8.1.4: an expiresAt before now → 400 BadRequestData.
    [Tags]    sub-create    5_8_1    since_v1.9.1
    ${subscription_id}=    Generate Random Subscription Id
    ${payload}=    Evaluate
    ...    {"id": "${subscription_id}", "type": "Subscription", "entities": [{"type": "Building"}], "expiresAt": "2000-01-01T00:00:00Z", "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}

5811_01_02 IsActive False Creates A Paused Subscription
    [Documentation]    5.8.1.4: isActive=false → initial status "paused";
    ...    the retrieved subscription must NOT report "active".
    [Tags]    sub-create    5_8_1    since_v1.9.1
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${payload}=    Evaluate
    ...    {"id": "${subscription_id}", "type": "Subscription", "entities": [{"type": "Building"}], "isActive": False, "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Strings    ${response.json()['status']}    paused
    [Teardown]    Delete Subscription    ${subscription_id}

5811_01_03 Subscription Without Id Gets A Generated URI
    [Documentation]    5.8.1.4: a subscription document without an id is
    ...    accepted and a new locally unique URI is generated (returned in
    ...    the Location header).
    [Tags]    sub-create    5_8_1    since_v1.9.1
    ${payload}=    Evaluate
    ...    {"type": "Subscription", "entities": [{"type": "Building"}], "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${location}=    Get From Dictionary    ${response.headers}    Location
    Should Contain    ${location}    urn:
    ${generated_id}=    Evaluate    "${location}".rsplit("/", 1)[-1]
    ${response}=    Retrieve Subscription
    ...    id=${generated_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    [Teardown]    Delete Subscription    ${generated_id}
