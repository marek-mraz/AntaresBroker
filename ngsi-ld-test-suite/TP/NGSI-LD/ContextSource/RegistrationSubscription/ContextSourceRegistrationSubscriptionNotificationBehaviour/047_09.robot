*** Settings ***
Documentation       Check if a context source registration subscription defines an "entities" member, a CsourceNotification will be triggered from context source registrations with information member matching the described "entities"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Create Initial Context Source Registration And Context Source Registration Subscription
Suite Teardown      Delete Created Context Source Registration And Context Source Registration Subscription


*** Variables ***
${context_source_registration_payload_file_path}=       csourceRegistrations/context-source-registration-building.jsonld
${subscription_payload_file_path}=                      csourceSubscriptions/subscription.jsonld
${update_fragment_file_path}=                           csourceRegistrations/fragments/context-source-registration-update-information.json


*** Test Cases ***
047_09_01 Receive cSourceNotification For No Longer Matching Context Source Registrations Providing Latest Information
    [Documentation]    Check if a context source registration subscription defines an "entities" member, a CsourceNotification will be triggered from context source registrations with information member matching the described "entities"
    [Tags]    csrsub-notification    5_11_7
    ${update_fragment}=    Load Test Sample    ${update_fragment_file_path}
    ${response}=    Update Context Source Registration    ${context_source_registration_id}    ${update_fragment}
    @{expected_context_source_registration_ids}=    Create List    ${context_source_registration_id}
    Wait for CSource notification and validate it
    ...    expected_subscription_id=${subscription_id}
    ...    expected_context_source_registration_ids=${expected_context_source_registration_ids}
    ...    expected_trigger_reason=noLongerMatching


*** Keywords ***
Create Initial Context Source Registration And Context Source Registration Subscription
    Start Local Server
    ${context_source_registration_id}=    Generate Random CSR Id
    ${subscription_id}=    Generate Random Subscription Id
    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_payload_file_path}
    ...    ${context_source_registration_id}
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ${create_csr_response}=    Create Context Source Registration    ${context_source_registration_payload}
    Check Response Status Code    201    ${create_csr_response.status_code}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    Set Suite Variable    ${context_source_registration_id}
    Set Suite Variable    ${subscription_id}
    # Drain the initial newlyMatching cSourceNotification sent on subscription creation (NGSI-LD 5.11.7),
    # so the test body validates the notification triggered by the registration update.
    Wait for notification

Delete Created Context Source Registration And Context Source Registration Subscription
    Stop Local Server
    Delete Context Source Registration    ${context_source_registration_id}
    Delete Context Source Registration Subscription    ${subscription_id}
