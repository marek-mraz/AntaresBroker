*** Settings ***
Documentation       Check that a notification is sent with changes for a VocabProperty when an entity is deleted
...                 and showChanges is true in the subscription

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-show-changes-entityDeleted.jsonld
${building_filename}=                   building-vocab-property-string.jsonld
${expectation_filename}=                building-deleted-vocab-property-deleted-entity.json


*** Test Cases ***
046_39_01 Delete Entity And Check Previous Values In Notification
    [Documentation]    Delete an entity and check the notification contains the previous values for a VocabProperty
    [Tags]    sub-notification    5_8_6    4_5_18    4_5_20    show_changes    since_v1.6.1

    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    ${expectation_filename}
    ...    ${notification}


*** Keywords ***
Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}    046_39
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Stop Local Server
