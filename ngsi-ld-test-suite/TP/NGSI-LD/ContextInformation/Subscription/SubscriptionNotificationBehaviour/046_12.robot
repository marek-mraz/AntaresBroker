*** Settings ***
Documentation       A Notification shall be sent (as mandated by each concrete binding and including any optional endpoint.info defined by clause 5.2.22) to the endpoint specified by the endpoint.uri member of the notification structure defined by clause 5.2.14

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
046_12_01 Check That lastNotification Is Updated
    [Documentation]    The status, lastNotification and lastSuccess members shall be updated with expected value and dates. This test will check these formats.
    [Tags]    sub-notification    5_8_6

    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    Wait for notification    timeout=${10}

    ${response}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}

    ${notification_info}=    Get Value From Json    ${response.json()}    $.notification

    Dictionary Should Contain Key    ${notification_info}[0]    status
    Should be Equal    ok    ${notification_info}[0][status]

    Dictionary Should Contain Key    ${notification_info}[0]    lastNotification
    ${last_notification_date}=    Parse Ngsild Date    ${notification_info}[0][lastNotification]
    Should Not Be Equal    ${last_notification_date}    ${None}

    Dictionary Should Contain Key    ${notification_info}[0]    lastSuccess
    ${last_success_date}=    Parse Ngsild Date    ${notification_info}[0][lastSuccess]
    Should Not Be Equal    ${last_success_date}    ${None}


*** Keywords ***
Before Test
    Start Local Server    ${notification_server_host}    ${notification_server_port}
    Add Initial Entity
    Sleep    1s
    Setup Initial Subscriptions

Add Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response1}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${create_response1.status_code}
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

Delete Initial Entity
    Delete Entity    ${entity_id}

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}
