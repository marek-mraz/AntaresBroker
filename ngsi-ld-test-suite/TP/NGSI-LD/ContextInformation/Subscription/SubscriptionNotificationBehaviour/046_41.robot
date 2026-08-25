*** Settings ***
Documentation       Check that no previous value is sent in notifications for attributeCreated triggers
...                 even when showChanges is true in the subscription

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Setup          Create Initial Subscription And Entity
Test Teardown       Delete Initial Subscription And Entity


*** Variables ***
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-show-changes-attribute-created.jsonld
${building_filename}=                   building-minimal.jsonld


*** Test Cases ***
046_41_01 Create And Check No Notification Is Sent
    [Documentation]    Create attribute and check that notification has no previous value
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1

    ${response}=    Update Entity Attributes    ${entity_id}    name-fragment.jsonld    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Check JSON Value Not In Response Body
    ...    $..previousValue
    ...    ${notification}[data]


*** Keywords ***
Create Initial Subscription And Entity
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}    046_41
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
