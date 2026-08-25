*** Settings ***
Documentation       Check that a notification is sent with changes (previous value) when an attribute is deleted
...                 and showChanges is true in the subscription

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity
Test Template       Delete Attribute And Check Previous Value In Notification


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-show-changes-attributeDeleted.jsonld
${building_filename}=                   building-different-attributes-types.jsonld


*** Test Cases ***    ATTRIBUTE_NAME    EXPECTATION_FILENAME
046_34_01 Show Changes For Deletion Of Property
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1
    name    entity-different-attributes-types-deleted-property.json
046_34_02 Show Changes For Deletion Of Relationship
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1
    locatedAt    entity-different-attributes-types-deleted-relationship.json
046_34_03 Show Changes For Deletion Of GeoProperty
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1
    location    entity-different-attributes-types-deleted-geoproperty.json
046_34_04 Show Changes For Deletion Of LanguageProperty
    [Tags]    sub-notification    5_8_6    4_5_18    show_changes    since_v1.6.1
    street    entity-different-attributes-types-deleted-language-property.json


*** Keywords ***
Delete Attribute And Check Previous Value In Notification
    [Documentation]    Delete an attribute from entity and check the notification contains the previous value
    [Arguments]    ${attribute_name}    ${expectation_filename}

    ${response}=    Delete Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_name}
    ...    datasetId=${EMPTY}
    ...    deleteAll=false
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    ${expectation_filename}
    ...    ${notification}

Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}    046_34
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
