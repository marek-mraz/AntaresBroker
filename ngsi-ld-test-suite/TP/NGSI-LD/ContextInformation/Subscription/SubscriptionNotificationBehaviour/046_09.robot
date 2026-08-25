*** Settings ***
Documentation       A Notification shall be sent (as mandated by each concrete binding and including any optional endpoint.receiverInfo defined by clause 5.2.22) to the endpoint specified by the endpoint.uri member of the notification structure defined by clause 5.2.14

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}       subscriptions/subscription-building-entities-active-endpoint-info.jsonld
${entity_building_filepath}             building-simple-attributes.jsonld
${fragment_filename}                    airQualityLevel-fragment.jsonld
${notification_server_send_url}         http://${notification_server_host}:${notification_server_port}/notify


*** Test Cases ***
046_09_01 Check That A Notification Is Sent To The Endpoint
    [Documentation]    A Notification shall be sent (as mandated by each concrete binding and including any optional endpoint.receiverInfo defined by clause 5.2.22) to the endpoint specified by the endpoint.uri member of the notification structure defined by clause 5.2.1
    [Tags]    sub-notification    5_8_6

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}
    Dictionary Should Contain Key    ${headers}    X-Additional-Key


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
