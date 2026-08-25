*** Settings ***
Documentation       Check that a notification is only sent if and only if the status is active

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Library             ${EXECDIR}/libraries/logUtils.py

Suite Setup         Before Suite
Suite Teardown      After Suite
Test Teardown       After Test


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-active.jsonld
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify
${entity_building_filepath}=            building-simple-attributes.jsonld
${fragment_filename}=                   airQualityLevel-fragment.jsonld


*** Test Cases ***
046_08_01 Check That A Notification Is Sent With All Attributes
    [Documentation]    The structure of the notification message shall be as mandated by clause 5.3.1. The absence of the notification.attributes member of a Subscription means that all Entity Attributes shall be included. All attributes are included
    [Tags]    sub-notification    5_8_6
    [Setup]    Setup Initial Subscriptions    ${False}

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification    ${5}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Dictionary Should Contain Key    ${notification}    data
    Should Not Be Empty    ${notification}[data]    Notification data should not be empty
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Should be Equal    Eiffel Tower    ${notification}[data][0][name][value]
    Dictionary Should Contain Key    ${notification}[data][0]    almostFull
    Dictionary Should Contain Key    ${notification}[data][0]    airQualityLevel
    Dictionary Should Contain Key    ${notification}[data][0]    subCategory

046_08_02 Check That A Notification Is Sent With All Attributes In Simplified Format
    [Documentation]    The structure of the notification message shall be as mandated by clause 5.3.1. The absence of the notification.attributes member of a Subscription means that all Entity Attributes shall be included    If the notification.format member value is "keyValues" then a simplified representation of the entities (as mandated by clause 4.5.3) shall be provided
    [Tags]    sub-notification    5_8_6
    [Setup]    Setup Initial Subscriptions    ${True}

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification    ${5}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Dictionary Should Contain Key    ${notification}    data
    Should Not Be Empty    ${notification}[data]    Notification data should not be empty
    Should be Equal    ${entity_id}    ${notification}[data][0][id]
    Should be Equal    Eiffel Tower    ${notification}[data][0][name]


*** Keywords ***
Before Suite
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Setup Initial Subscriptions
    [Arguments]    ${change_json}
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}

    IF    "${change_json}" == "${True}"
        ${subscription_payload}=    Update Value To JSON
        ...    ${subscription_payload}
        ...    $..notification['format']
        ...    keyValues
    END

    ${subscription_payload}=    Set Entity Id In Subscription    ${subscription_payload}    ${entity_id}
    Set Test Variable    ${entity_id}
    Set Test Variable    ${subscription_id}

    ${create_response1}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Sleep    1s
    ${create_response2}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    Sleep    1s

After Test
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}

After Suite
    Stop Local Server
