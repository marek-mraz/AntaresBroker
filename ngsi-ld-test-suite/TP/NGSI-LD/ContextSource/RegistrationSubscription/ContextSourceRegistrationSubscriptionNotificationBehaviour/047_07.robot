*** Settings ***
Documentation       Check that a cSourceNotification shall only be sent if and only if the status of the corresponding subscription is active, neither paused or expired

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Test Setup          Setup Initial Context Source Registration Subscription
Test Teardown       Delete Created Context Source Registration And Subscription
Test Template       Do Not Receive cSourceNotification If Subscription Status Is Not Active


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration.jsonld
${subscription_payload_file_path}=                      csourceSubscriptions/subscription.jsonld


*** Test Cases ***    FILEPATH
047_07_01 PausedSubscription
    [Tags]    csrsub-notification    5_11_7
    csourceSubscriptions/fragments/subscription-isActive-update.json
047_07_02 ExpiredSubscription
    [Tags]    csrsub-notification    5_11_7
    csourceSubscriptions/fragments/subscription-expiresAt-update.json


*** Keywords ***
Do Not Receive cSourceNotification If Subscription Status Is Not Active
    [Documentation]    Check that a cSourceNotification shall only be sent if and only if the status of the corresponding subscription is active, neither paused or expired
    [Arguments]    ${filepath}
    ${subscription_update_fragment}=    Load Test Sample    ${filepath}
    ${response}=    Update Context Source Registration Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment}

    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    Set Suite Variable    ${context_source_registration_id}
    ${response1}=    Create Context Source Registration    ${context_source_registration_payload}
    Wait for no notification

Setup Initial Context Source Registration Subscription
    Start Local Server
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ${create_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}

Delete Created Context Source Registration And Subscription
    Delete Context Source Registration    ${context_source_registration_id}
    Delete Context Source Registration Subscription    ${subscription_id}
    Stop Local Server
