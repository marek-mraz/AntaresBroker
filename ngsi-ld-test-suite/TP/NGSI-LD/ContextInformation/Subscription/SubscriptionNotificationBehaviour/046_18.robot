*** Settings ***
Documentation       The system generated attributes are not included in the response payload body of a notification if sysAttrs parameter is set to false.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}       subscriptions/subscription-with-false-sysAttrs-parameter.jsonld
${entity_building_filepath}             building-simple-attributes.jsonld
${fragment_filename}                    airQualityLevel-fragment.jsonld
${notification_server_send_url}         http://${notification_server_host}:${notification_server_port}/notify


*** Test Cases ***
046_18_01 Check That The Response Payload Body Does Not Contain The System Generated Attributes If sysAttrs Parameter Is Set To False
    [Documentation]    The system generated attributes are not included in the response payload body of a notification if sysAttrs parameter is set to false.
    [Tags]    sub-notification    5_8_6    since_v1.6.1

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    ${notification_payload}=    Get Request Body

    ${notification}=    Evaluate    json.loads('''${notification_payload}''')    json

    Dictionary Should Not Contain Key    ${notification}[data][0]    createdAt
    Dictionary Should Not Contain Key    ${notification}[data][0]    modifiedAt


*** Keywords ***
Before Test
    Start Local Server    ${notification_server_host}    ${notification_server_port}
    Setup Initial Subscriptions

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

    Create Entity    ${entity_building_filepath}    ${entity_id}
    Sleep    1s
    Create Subscription From Subscription Payload    ${subscription_payload}    ${CONTENT_TYPE_LD_JSON}
    Sleep    1s

After Test
    Delete Initial Subscriptions
    Delete Initial Entity
    Stop Local Server

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
