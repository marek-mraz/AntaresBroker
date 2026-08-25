*** Settings ***
Documentation       Check that a notification is sent with flat linked entity when linking entity is updated

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity With Linked Entity
Test Teardown       Delete Initial Subscription And Entity With Linked Entity


*** Variables ***
${linking_entity_filename}=             building-relationship.jsonld
${linked_entity_filename}=              city-minimal.jsonld
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-join-flat.jsonld
${fragment_filename}=                   name-fragment.jsonld
${entity_expectation_file_path}=        entity-created-name-attribute-join-flat.json


*** Test Cases ***
046_23_01 Check That A Notification Is Sent With Linked Entity In Flat Representation
    [Documentation]    Create an attribute in linking entity and check the notification contains linked entity
    [Tags]    sub-notification    4_5_23    5_8_6    since_v1.8.1

    ${response}=    Update Entity Attributes    ${linking_entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    ${entity_expectation_file_path}
    ...    ${notification}


*** Keywords ***
Create Initial Subscription And Entity With Linked Entity
    Create Subscription And Entity With Linked Entity
    ...    ${subscription_payload_file_path}
    ...    ${linking_entity_filename}
    ...    046_23:EiffelTower
    ...    ${linked_entity_filename}
    ...    Paris
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity With Linked Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${linking_entity_id}
    Delete Entity    ${linked_entity_id}
    Stop Local Server
