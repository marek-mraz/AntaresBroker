*** Settings ***
Documentation       If a subscription has a datasetId member instances should be filtered based on that datasetId.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}       subscriptions/subscription-building-with-datasetId.jsonld
${entity_building_filepath}             building-multi-instances-attributes.jsonld
${fragment_filename}                    building-name-fragment.jsonld
${notification_server_send_url}         http://${notification_server_host}:${notification_server_port}/notify
${entity_expectation_file_path}         entity-with-datasetid-046-19.jsonld


*** Test Cases ***
046_19 Check That Only The Attribute Instances That Match The datasetId Member Are Included In The Entity In The Notification
    [Documentation]    If a subscription has a datasetId member instances should be filtered based on that datasetId.
    [Tags]    sub-notification    4_5_5    5_8_6    since_v1.8.1

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Should be Equal    ${entity_id}    ${notification}[data][0][id]

    Check Notification Containing Entity Element
    ...    ${entity_expectation_file_path}
    ...    ${notification}


*** Keywords ***
Before Test
    Start Local Server    ${notification_server_host}    ${notification_server_port}
    Add Initial Entity
    Sleep    1s
    Setup Initial Subscription

Add Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${entity_id}

Setup Initial Subscription
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
    Delete Initial Subscription
    Delete Initial Entity
    Stop Local Server

Delete Initial Subscription
    Delete Subscription    ${subscription_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
