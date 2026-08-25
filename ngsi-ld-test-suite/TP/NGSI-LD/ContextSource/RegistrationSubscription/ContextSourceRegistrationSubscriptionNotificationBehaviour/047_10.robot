*** Settings ***
Documentation       Check if a context source registration subscription defines temporalQ member with timeproperty observedAt, the temporal query is matched against the observationInterval of matching context source registrations

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Test Setup          Setup Initial Context Source Registration Subscription
Test Teardown       Delete Created Context Source Registration And Subscription


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-observationInterval.jsonld
${subscription_payload_file_path}=                      csourceSubscriptions/subscription-temporalQ-observedAt.jsonld


*** Test Cases ***
047_10_01 Receive cSourceNotification For Matching Context Source Registrations On Observation Interval
    [Documentation]    Check if a context source registration subscription defines temporalQ member with timeproperty observedAt, the temporal query is matched against the observationInterval of matching context source registrations
    [Tags]    csrsub-notification    5_11_7
    ${context_source_registration_id}=    Generate Random CSR Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    Set Suite Variable    ${context_source_registration_id}
    ${response}=    Create Context Source Registration    ${context_source_registration_payload}
    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Wait for CSource notification and validate it
    ...    expected_subscription_id=${subscription_id}
    ...    expected_context_source_registration_ids=${expected_context_source_registration_ids}
    ...    expected_trigger_reason=newlyMatching


*** Keywords ***
Setup Initial Context Source Registration Subscription
    Start Local Server
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    Set Suite Variable    ${subscription_id}

Delete Created Context Source Registration And Subscription
    Stop Local Server
    Delete Context Source Registration    ${context_source_registration_id}
    Delete Context Source Registration Subscription    ${subscription_id}
