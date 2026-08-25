*** Settings ***
Documentation       Check that a notification is sent when an entity is created and entityCreated notification trigger is configured

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Setup Server And Subscriptions
Suite Teardown      Delete Server And Subscriptions


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-entityCreated.jsonld
${building_filename}=                   building-location-attribute.jsonld
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify


*** Test Cases ***
046_06_01 Check That A Notification Is Sent With All Matching Entities
    [Documentation]    Check that a notification is sent when an entity is created and entityCreated notification trigger is configured
    [Tags]    sub-notification    5_8_6    since_v1.6.1
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity Selecting Content Type
    ...    ${building_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}
    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Should be Equal    ${entity_id}    ${notification}[data][0][id]


*** Keywords ***
Setup Server And Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}
    Set Suite Variable    ${subscription_id}
    Start Local Server    ${notification_server_host}    ${notification_server_port}

    ${create_response}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Sleep    1s

Delete Server And Subscriptions
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
