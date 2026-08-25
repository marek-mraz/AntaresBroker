*** Settings ***
Documentation       Check that a notification is sent with changes when a VocabProperty is updated

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-show-changes.jsonld
${building_filename}=                   building-vocab-property-string.jsonld
${fragment_filename}=                   vocab-property-fragment.jsonld
${expectation_filename}=                building-previous-vocab-property.json


*** Test Cases ***
046_33_01 Update Vocab Property And Check Previous Value In Notification
    [Documentation]    Update the VocabProperty in the entity and check the notification contains the previous value
    [Tags]    sub-notification    5_8_6    4_5_20    show_changes    since_v1.6.1

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    ${expectation_filename}
    ...    ${notification}


*** Keywords ***
Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}    046_33
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
