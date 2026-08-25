*** Settings ***
Documentation       If a Subscription defines a timeInterval member, a Notification shall be sent periodically, when the time interval (in seconds) specified in such value field is reached, regardless of Attribute changes.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-timeInterval.jsonld
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify
${entity_building_filepath}=            building-simple-attributes.jsonld


*** Test Cases ***
046_02_01 Check That A Notification Is Sent On The timeInterval
    [Documentation]    If a Subscription defines a timeInterval member, a Notification shall be sent periodically, when the time interval (in seconds) specified in such value field is reached, regardless of Attribute changes.
    [Tags]    sub-notification    5_8_6    brokerok-harness
    ${response}=    Setup Initial Subscriptions

    ${notification}    ${headers}=    Wait for notification    timeout=${15}

    ${notification}    ${headers}=    Wait for notification    timeout=${15}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Dictionary Should Contain Key    ${notification}    data
    Should Not Be Empty    ${notification}[data]    Notification data should not be empty
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Should be True
    ...    '${notification}[data][0][airQualityLevel][value]'=='4.0' or '${notification}[data][0][airQualityLevel][value]'=='4'
    Should be Equal    Eiffel Tower    ${notification}[data][0][name][value]


*** Keywords ***
Before Test
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Setup Initial Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}
    ${subscription_payload}=    Set Entity Id In Subscription    ${subscription_payload}    ${entity_id}
    Set Suite Variable    ${entity_id}
    Set Suite Variable    ${subscription_id}

    ${create_response1}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Sleep    1s
    ${create_response2}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    Sleep    1s

After Test
    Delete Initial Subscriptions
    Delete Initial Entity
    Stop Local Server

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
