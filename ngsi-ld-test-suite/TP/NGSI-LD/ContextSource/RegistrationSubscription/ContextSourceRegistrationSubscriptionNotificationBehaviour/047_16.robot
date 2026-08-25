*** Settings ***
Documentation       Check if one updates a context source registration subscription, a CsourceNotification will be sent with all currently matching context source registrations

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Test Setup          Create Initial Context Source Registrations And Context Source Registration Subscription
Test Teardown       Delete Created Context Source Registrations And Context Source Registration Subscription
Test Template       Receive cSourceNotification For Newly Matching Context Source Registrations


*** Variables ***
${first_context_source_registration_payload_file_path}=     csourceRegistrations/context-source-registration-vehicle-complete.jsonld
${second_context_source_registration_payload_file_path}=    csourceRegistrations/context-source-registration-bus-entities.jsonld
${subscription_payload_file_path}=                          csourceSubscriptions/subscription.jsonld


*** Test Cases ***    FILEPATH    NOTIFICATION_CSR_IDS
047_16_01 MatchFirstContextSourceRegistration
    [Tags]    csrsub-notification    5_11_7
    csourceSubscriptions/fragments/subscription-vehicle-entities.json    ${first_context_source_registration_id}
047_16_02 MatchSecondContextSourceRegistration
    [Tags]    csrsub-notification    5_11_7
    csourceSubscriptions/fragments/subscription-bus-entities.json    ${second_context_source_registration_id}
047_16_03 MatchBothContextSourceRegistrations
    [Tags]    csrsub-notification    5_11_7
    csourceSubscriptions/fragments/subscription-vehicle-and-bus-entities.json    ${first_context_source_registration_id}    ${second_context_source_registration_id}


*** Keywords ***
Receive cSourceNotification For Newly Matching Context Source Registrations
    [Documentation]    Check if one updates a context source registration subscription, a CsourceNotification will be sent with all currently matching context source registrations
    [Arguments]    ${filepath}    @{notification_csr_ids}
    ${subscription_update_fragment}=    Load Test Sample    ${filepath}
    ${response}=    Update Context Source Registration Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment}
    Wait for CSource notification and validate it
    ...    expected_subscription_id=${subscription_id}
    ...    expected_context_source_registration_ids=${notification_csr_ids}
    ...    expected_trigger_reason=newlyMatching

Create Initial Context Source Registrations And Context Source Registration Subscription
    Start Local Server
    ${subscription_id}=    Generate Random Subscription Id
    ${first_context_source_registration_id}=    Generate Random CSR Id
    ${second_context_source_registration_id}=    Generate Random CSR Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ${first_context_source_registration_payload}=    Load Test Sample
    ...    ${first_context_source_registration_payload_file_path}
    ...    ${first_context_source_registration_id}
    ${second_context_source_registration_payload}=    Load Test Sample
    ...    ${second_context_source_registration_payload_file_path}
    ...    ${second_context_source_registration_id}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    ${create_csr_response1}=    Create Context Source Registration    ${first_context_source_registration_payload}
    Check Response Status Code    201    ${create_csr_response1.status_code}
    ${create_csr_response2}=    Create Context Source Registration    ${second_context_source_registration_payload}
    Check Response Status Code    201    ${create_csr_response2.status_code}
    Set Test Variable    ${subscription_id}
    Set Test Variable    ${first_context_source_registration_id}
    Set Test Variable    ${second_context_source_registration_id}

Delete Created Context Source Registrations And Context Source Registration Subscription
    Stop Local Server
    Delete Context Source Registration Subscription    ${subscription_id}
    Delete Context Source Registration    ${first_context_source_registration_id}
    Delete Context Source Registration    ${second_context_source_registration_id}
