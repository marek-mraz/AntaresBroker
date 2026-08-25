*** Settings ***
Documentation       Check if a context source registrations subscription does not define a geoproperty in the geoQ member, a CsourceNotification will be triggered from matching context source registrations with a matching location member

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Test Setup          Setup Initial Context Source Registration Subscription
Test Teardown       Delete Created Context Source Registration And Subscription


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-location.jsonld
${subscription_payload_file_path}=                      csourceSubscriptions/subscription-geoQ-without-geoproperty.jsonld


*** Test Cases ***
047_15_01 Receive cSourceNotification For Matching Context Source Registrations On Location As Default
    [Documentation]    Check if a context source registrations subscription does not define a geoproperty in the geoQ member, a CsourceNotification will be triggered from matching context source registrations with a matching location member
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
