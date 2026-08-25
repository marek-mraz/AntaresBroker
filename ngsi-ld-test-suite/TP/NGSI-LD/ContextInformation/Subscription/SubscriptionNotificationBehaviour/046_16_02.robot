*** Settings ***
Documentation       If a subscription defines an entity type selection query, a notification shall be sent whenever an entity matches the query.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-type-selection.jsonld
${notification_server_send_url}=        http://${notification_server_host}:${notification_server_port}/notify
${entity_vehicle_filepath}=             vehicle-simple-attributes.jsonld
${content_type}=                        application/ld+json


*** Test Cases ***
046_16_02 Check That A Notification Is Not Sent If The Entity Type Does Not Match The Entity Type Selection
    [Documentation]    If a subscription defines an entity type selection query, a notification shall not be sent if the entity type does not match the query
    [Tags]    sub-notification    5_8_6    since_v1.5.1

    ${vehicle_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_vehicle_filepath}
    ...    ${vehicle_id}
    ...    ${content_type}
    Set Suite Variable    ${vehicle_id}

    Wait for no notification


*** Keywords ***
Setup Initial Subscription
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${notification_server_send_url}
    ${create_response}=    Create Subscription From Subscription Payload
    ...    ${subscription_payload}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}

Before Test
    Start Local Server    ${notification_server_host}    ${notification_server_port}
    Sleep    1s
    Setup Initial Subscription

After Test
    Delete Subscription    ${subscription_id}
    Delete Entity    ${vehicle_id}
    Stop Local Server
