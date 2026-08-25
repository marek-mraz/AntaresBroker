*** Settings ***
Documentation       Check that if the created context source registration subscription does not define a timeInterval member, a cSourceNotification, with the appropriate trigger reason in the "triggerReason" member, will be sent initially on subscription and whenever there is a change of a matching Context Source Registration

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Test Setup          Create Initial Context Source Registration and Context Source Registration Subscription
Test Teardown       Delete Created Context Source Registration And Context Source Registration Subscription


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-building.jsonld
${subscription_payload_file_path}=                      csourceSubscriptions/subscription.jsonld
${update_fragment_file_path}=                           csourceRegistrations/fragments/context-source-registration-update.json


*** Test Cases ***
047_02_01 Receive cSourceNotification Initially On Subscription And Whenever There Is A Change Of A Matching Context Source Registration
    [Documentation]    Check that if the created context source registration subscription does not define a timeInterval member, a cSourceNotification, with the appropriate trigger reason in the "triggerReason" member, will be sent initially on subscription and whenever there is a change of a matching Context Source Registration
    [Tags]    csrsub-notification    5_11_7
    ${update_fragment}=    Load Test Sample    ${update_fragment_file_path}
    ${response}=    Update Context Source Registration    ${context_source_registration_id}    ${update_fragment}
    Wait for CSource notification and validate it
    ...    expected_subscription_id=${subscription_id}
    ...    expected_context_source_registration_ids=${expected_context_source_registration_ids}
    ...    expected_trigger_reason=updated


*** Keywords ***
Create Initial Context Source Registration and Context Source Registration Subscription
    Start Local Server

    ${context_source_registration_id}=    Generate Random CSR Id
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${context_source_registration_id}
    Set Suite Variable    ${subscription_id}

    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}

    ${create_csr_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_csr_response.status_code}

    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Set Suite Variable    ${expected_context_source_registration_ids}
    Wait for CSource notification and validate it
    ...    expected_subscription_id=${subscription_id}
    ...    expected_context_source_registration_ids=${expected_context_source_registration_ids}
    ...    expected_trigger_reason=newlyMatching

Delete Created Context Source Registration And Context Source Registration Subscription
    Stop Local Server
    Delete Context Source Registration    ${context_source_registration_id}
    Delete Context Source Registration Subscription    ${subscription_id}
