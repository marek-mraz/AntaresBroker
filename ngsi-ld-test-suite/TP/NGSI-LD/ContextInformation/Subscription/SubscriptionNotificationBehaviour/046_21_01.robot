*** Settings ***
Documentation       Check that a notification is sent when an entity is deleted
...                 and entityDeleted notification trigger is configured

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-entityDeleted.jsonld
${building_filename}=                   building-location-attribute.jsonld


*** Test Cases ***
046_21_01 Check That A Notification Is Sent With Matching Entity
    [Documentation]    Delete an entity and check the received notification
    [Tags]    sub-notification    5_8_6    since_v1.6.1

    ${response}=    Delete Entity
    ...    id=${entity_id}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Length Should Be    ${notification}[data]    ${1}
    ${notified_entity}=    Set Variable    ${notification}[data][0]
    Length Should Be    ${notified_entity}    ${3}
    Should be Equal    ${entity_id}    ${notified_entity}[id]
    Should Have Value In Json
    ...    json_object=${notified_entity}
    ...    json_path=['deletedAt']


*** Keywords ***
Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Stop Local Server
