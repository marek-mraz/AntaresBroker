*** Settings ***
Documentation       Check that if a cSourceNotification is sent successfully to the "endpoint" member, the "notification.timesSent" member shall be incremented by one and the "notification.lastSuccess" and "notification.lastNotification" members shall be updated with the current timestamp and the status of the context source registration subscription shall be updated to "ok"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Test Setup          Setup Initial Context Source Registration Subscription
Test Teardown       Delete Created Context Source Registration And Subscription


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-building.jsonld
${subscription_payload_file_path}=                      csourceSubscriptions/subscription.jsonld
${notification_expectation_file_path}=                  notifications/expectations/1-timesSent-ok.json


*** Test Cases ***
047_05_01 If A cSourceNotification Is Successfully Sent The Notification Member Shall Be Updated
    [Documentation]    Check that if a cSourceNotification is sent successfully to the "endpoint" member, the "notification.timesSent" member shall be incremented by one and the "notification.lastSuccess" and "notification.lastNotification" members shall be updated with the current timestamp and the status of the context source registration subscription shall be updated to "ok"
    [Tags]    csrsub-notification    5_11_7
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    Set Suite Variable    ${context_source_registration_id}
    ${create_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Wait for notification
    ${response}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}
    @{expected_notification_additional_members}=    Create List    lastNotification    lastSuccess
    Check NotificationParams
    ...    ${notification_expectation_file_path}
    ...    ${expected_notification_additional_members}
    ...    ${response.json()}


*** Keywords ***
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
    Stop Local Server
    Delete Context Source Registration    ${context_source_registration_id}
    Delete Context Source Registration Subscription    ${subscription_id}
