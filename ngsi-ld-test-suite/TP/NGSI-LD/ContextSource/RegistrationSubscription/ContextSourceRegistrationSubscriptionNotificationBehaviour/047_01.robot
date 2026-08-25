*** Settings ***
Documentation       Check that if the created context source registration subscription defines a timeInterval member, a cSourceNotification will be sent periodically, initially on subscription and when the time interval is reached

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Setup Initial Context Source Registration
Suite Teardown      Delete Created Context Source Registration And Subscription


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-building.jsonld
${subscription_payload_file_path}=                      csourceSubscriptions/subscription-timeInterval.jsonld


*** Test Cases ***
047_01_01 Receive cSourceNotification Periodically And Initially On Subscription
    [Documentation]    Check that if the created context source registration subscription defines a timeInterval member, a cSourceNotification will be sent periodically, initially on subscription and when the time interval is reached
    [Tags]    csrsub-notification    5_11_7
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    Set Suite Variable    ${subscription_id}
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}
    # Wait for 15 seconds to check if another notification was sent
    Wait for notification    timeout=${15}


*** Keywords ***
Setup Initial Context Source Registration
    Start Local Server
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    ${create_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${context_source_registration_id}

Delete Created Context Source Registration And Subscription
    Stop Local Server
    Delete Context Source Registration    ${context_source_registration_id}
    Delete Context Source Registration Subscription    ${subscription_id}
