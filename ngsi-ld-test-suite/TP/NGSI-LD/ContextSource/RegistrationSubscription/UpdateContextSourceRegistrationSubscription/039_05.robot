*** Settings ***
Documentation       Check that one cannot update a context source registration subscription with an invalid request body (invalid JSON document)

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration Subscriptions
Test Teardown       Delete Initial Context Source Registration Subscriptions


*** Variables ***
${subscription_payload_file_path}=              csourceSubscriptions/subscription.jsonld
${subscription_update_fragment_file_path}=      csourceSubscriptions/fragments/subscription-update-invalid-json.json


*** Test Cases ***
039_05_01 Update Context Source Registration Subscription With Invalid JSON Fragment
    [Documentation]    Check that one cannot update a context source registration subscription with an invalid request body (invalid JSON document)
    [Tags]    csrsub-update    5_11_3
    ${response}=    Update Context Source Registration Subscription From File
    ...    ${subscription_id}
    ...    ${subscription_update_fragment_file_path}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    response_body=${response.json()}
    ...    problem_type=${ERROR_TYPE_INVALID_REQUEST}


*** Keywords ***
Setup Initial Context Source Registration Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    Create Context Source Registration Subscription    ${subscription_payload}
    Set Suite Variable    ${subscription_id}

Delete Initial Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${subscription_id}
