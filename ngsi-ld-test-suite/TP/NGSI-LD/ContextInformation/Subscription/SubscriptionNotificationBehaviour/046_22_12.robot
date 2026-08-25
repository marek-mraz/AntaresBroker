*** Settings ***
Documentation       Check that a notification is sent in normalized format when an entity is deleted
...                 and attributeDeleted notification trigger is configured for a matching attribute in deleted entity

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld
${building_filename}=                   building-simple-attributes.jsonld
${entity_expectation_file_path}=        entity-deleted-name-attribute-on-entity-deleted-normalized.json


*** Test Cases ***
046_22_12 Check That A Notification Is Sent With Matching Entity
    [Documentation]    Delete an entity with a matching attribute and check the received notification
    [Tags]    sub-notification    5_8_6    since_v1.6.1

    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Length Should Be    ${notification}[data]    ${1}
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Check Notification Containing Entity Element
    ...    ${entity_expectation_file_path}
    ...    ${notification}


*** Keywords ***
Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
