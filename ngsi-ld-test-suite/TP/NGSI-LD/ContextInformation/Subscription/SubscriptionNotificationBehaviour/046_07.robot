*** Settings ***
Documentation       Check that a notification is only sent if and only if the status is active

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Suite
Suite Teardown      After Suite
Test Teardown       After Test


*** Variables ***
${subscription_payload_file_path}                           subscriptions/subscription-building-entities-active.jsonld
${subscription_payload_file_path_notificationAttributes}    subscriptions/subscription-building-entities-active-notificationAttributes.jsonld
${subscription_payload_file_path_default_context}           subscriptions/subscription-building-entities-active-default-context.jsonld
${notification_server_send_url}                             http://${notification_server_host}:${notification_server_port}/notify
${entity_building_filepath}                                 building-simple-attributes.jsonld
${fragment_filename}                                        airQualityLevel-fragment.jsonld
${date_format}                                              %Y-%m-%dT%H:%M:%SZ
${date_format_with_millis}                                  %Y-%m-%dT%H:%M:%S.%fZ


*** Test Cases ***
046_07_01 Check Notification Structure
    [Documentation]    The structure of the notification message shall be as mandated by clause 5.3. Valid notification with attributes as stated above
    [Tags]    sub-notification    5_8_6
    [Setup]    Setup Initial Subscriptions    ${subscription_payload_file_path}

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification
    Should Be Equal    ${notification}[type]    Notification
    Should Be Equal    ${notification}[subscriptionId]    ${subscription_id}
    ${notified_at_date}=    Parse Ngsild Date    ${notification}[notifiedAt]
    Should Not Be Equal    ${notified_at_date}    ${None}

046_07_02 Check Correct Attributes Are Included
    [Documentation]    The structure of the notification message shall be as mandated by clause 5.3.    The Entity Attributes included (Properties or Relationships) shall be those specified by the notification.attributes member in the Subscription data type (clause 5.2.12).
    [Tags]    sub-notification    5_8_6
    [Setup]    Setup Initial Subscriptions    ${subscription_payload_file_path_notificationAttributes}

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification
    Should Be Equal    ${notification}[type]    Notification
    Should Be Equal    ${notification}[subscriptionId]    ${subscription_id}
    ${notified_at_date}=    Parse Ngsild Date    ${notification}[notifiedAt]
    Should Not Be Equal    ${notified_at_date}    ${None}
    Dictionary Should Not Contain Key    ${notification}[data][0]    name
    Dictionary Should Contain Key    ${notification}[data][0]    airQualityLevel

046_07_03 Check URI Expansion Is Observed
    [Documentation]    The structure of the notification message shall be as mandated by clause 5.3.    URI expansion shall be observed (clause 5.5.7).
    [Tags]    sub-notification    5_8_6
    [Setup]    Setup Initial Subscriptions    ${subscription_payload_file_path_default_context}

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification
    Should Be Equal    ${notification}[type]    Notification
    Should Be Equal    ${notification}[subscriptionId]    ${subscription_id}
    ${notified_at_date}=    Parse Ngsild Date    ${notification}[notifiedAt]
    Should Not Be Equal    ${notified_at_date}    ${None}
    Dictionary Should Contain Key    ${notification}[data][0]    https://ngsi-ld-test-suite/context#airQualityLevel


*** Keywords ***
Before Suite
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Setup Initial Subscriptions
    [Arguments]    ${subscription_payload_path}
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}
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
