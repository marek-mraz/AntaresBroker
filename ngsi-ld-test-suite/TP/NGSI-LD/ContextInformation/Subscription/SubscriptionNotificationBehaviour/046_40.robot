*** Settings ***
Documentation       Check that no previous value is sent in notifications for entityCreated trigger
...                 even when showChanges is true in the subscription

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Test Setup          Create Initial Subscription
Test Teardown       Delete Initial Subscription


*** Variables ***
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-show-changes-entity-created.jsonld
${building_filename}=                   building-simple-attributes.jsonld


*** Test Cases ***
046_40_01 Create Entity And Check No Previous Value In Notification
    [Documentation]    Create entity and check that notification has no previous value
    [Tags]    sub-notification    5_8_6    show_changes    since_v1.6.1

    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Create Entity    ${building_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${entity_id}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Check JSON Value Not In Response Body
    ...    $..previousValue
    ...    ${notification}[data]


*** Keywords ***
Create Initial Subscription
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}
    ${create_response}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${subscription_id}
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Initial Subscription
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
