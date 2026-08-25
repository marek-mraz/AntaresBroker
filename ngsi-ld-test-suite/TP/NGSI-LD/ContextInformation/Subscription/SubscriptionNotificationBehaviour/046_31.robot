*** Settings ***
Documentation       Check that a notification is sent with changes when an attribute is updated

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity
Test Template       Update Attribute And Check Previous Value In Notification


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-show-changes.jsonld
${building_filename}=                   building-different-attributes-types.jsonld


*** Test Cases ***    FRAGMENT_FILENAME    EXPECTATION_FILENAME
046_31_01 Show Changes For An Update Of Property
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1
    name-fragment.jsonld    entity-different-attributes-types-previous-property.json
046_31_02 Show Changes For An Update Of Relationship
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1
    locatedAt-fragment.jsonld    entity-different-attributes-types-previous-relationship.json
046_31_03 Show Changes For An Update Of GeoProperty
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1
    location-fragment.jsonld    entity-different-attributes-types-previous-geoproperty.json
046_31_04 Show Changes For An Update Of LanguageProperty
    [Tags]    sub-notification    5_8_6    4_5_18    show_changes    since_v1.6.1
    street-language-property-fragment.jsonld    entity-different-attributes-types-previous-language-property.json


*** Keywords ***
Update Attribute And Check Previous Value In Notification
    [Documentation]    Update an attribute in entity and check the notification contains the previous value
    [Arguments]    ${fragment_filename}    ${expectation_filename}

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    ${expectation_filename}
    ...    ${notification}

Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}    046_31
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
