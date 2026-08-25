*** Settings ***
Documentation       Check that a notification is only sent if and only if the status is active

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-active.jsonld
${entity_building_filepath}=            building-simple-attributes.jsonld
${fragment_filename}=                   airQualityLevel-fragment.jsonld
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify


*** Test Cases ***
046_01_01 Check That A Notification Is Only Sent If Status Is Active
    [Documentation]    Check that a notification is only sent if and only if the status is active
    [Tags]    sub-notification    5_8_6

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Should be Equal    ${5}    ${notification}[data][0][airQualityLevel][value]


*** Keywords ***
Before Test
    Start Local Server    ${notification_server_host}    ${notification_server_port}
    Add Initial Entity
    Sleep    1s
    Setup Initial Subscriptions

Add Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${entity_id}

Setup Initial Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}
    ${subscription_payload}=    Set Entity Id In Subscription    ${subscription_payload}    ${entity_id}
    ${create_response}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}

After Test
    Delete Initial Subscriptions
    Delete Initial Entity
    Stop Local Server

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
