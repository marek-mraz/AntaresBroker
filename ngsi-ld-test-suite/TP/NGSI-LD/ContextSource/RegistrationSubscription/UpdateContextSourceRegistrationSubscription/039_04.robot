*** Settings ***
Documentation       Check that one cannot update a context source registration subscription with a fragment that does not meet the data types and restrictions expressed by clause 5.2.12

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration Subscriptions
Test Teardown       Delete Initial Context Source Registration Subscriptions
Test Template       Update Context Source Registration Subscription With Invalid Fragment


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription.jsonld


*** Test Cases ***    FILEPATH
039_04_01 InvalidType
    [Tags]    csrsub-update    5_11_3
    csourceSubscriptions/fragments/subscription-update-invalid-type.json
039_04_02 InvalidNotification
    [Tags]    csrsub-update    5_11_3
    csourceSubscriptions/fragments/subscription-update-invalid-notification.json


*** Keywords ***
Update Context Source Registration Subscription With Invalid Fragment
    [Documentation]    Check that one cannot update a context source registration subscription with a fragment that does not meet the data types and restrictions expressed by clause 5.2.12
    [Arguments]    ${filepath}
    ${subscription_update_fragment}=    Load Test Sample    ${filepath}
    ${response}=    Update Context Source Registration Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    response_body=${response.json()}
    ...    problem_type=${ERROR_TYPE_BAD_REQUEST_DATA}

Setup Initial Context Source Registration Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    Set Test Variable    ${subscription_id}

Delete Initial Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${subscription_id}
