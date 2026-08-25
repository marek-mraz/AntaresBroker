*** Settings ***
Documentation       Check that a notification is sent in normalized format when a relationship is deleted via NGSI-LD Null
...                 in a Merge Entity operation and attributeDeleted notification trigger is configured for this specific relationship

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-attributeDeleted-relationship-normalized.jsonld
${building_filename}=                   building-different-attributes-types.jsonld
${entity_expectation_file_path}=        entity-different-attributes-types-deleted-locatedAt-attribute-normalized.json


*** Test Cases ***
046_22_10_01 Check That A Notification Is Sent With Matching Entity When Deleting Via Merge Entity Operation
    [Documentation]    Delete a relationship via Merge Entity operation and check the received notification
    [Tags]    sub-notification    5_8_6    since_v1.6.1

    ${response}=    Merge Entity
    ...    entity_id=${entity_id}
    ...    entity_filename=fragmentEntities/ngsild-null/building-null-relationship.jsonld
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Length Should Be    ${notification}[data]    ${1}
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Check Notification Containing Entity Element
    ...    ${entity_expectation_file_path}
    ...    ${notification}

046_22_10_02 Check That A Notification Is Sent With Matching Entity When Deleting Via Update Attributes Operation
    [Documentation]    Delete a relationship via Update Attributes operation and check the received notification
    [Tags]    sub-notification    5_8_6    since_v1.6.1

    ${response}=    Update Entity Attributes
    ...    id=${entity_id}
    ...    fragment_filename=ngsild-null/null-relationship.jsonld
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Length Should Be    ${notification}[data]    ${1}
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Check Notification Containing Entity Element
    ...    ${entity_expectation_file_path}
    ...    ${notification}

046_22_10_03 Check That A Notification Is Sent With Matching Entity When Deleting Via Partial Attribute Update Operation
    [Documentation]    Delete a relationship via Update Partial Attribute Update operation and check the received notification
    [Tags]    sub-notification    5_8_6    since_v1.6.1

    ${response}=    Partial Update Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=locatedAt
    ...    fragment_filename=ngsild-null/null-relationship-fragment.jsonld
    ...    content_type=${CONTENT_TYPE_LD_JSON}
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
