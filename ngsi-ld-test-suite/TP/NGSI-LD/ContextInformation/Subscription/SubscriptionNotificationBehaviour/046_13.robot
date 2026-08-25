*** Settings ***
Documentation       If the response to the notification request is different than 200 OK then implementations shall: Update notification.lastFailure with a timestamp representing the current date and time, update notification.status to "failed"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource

Suite Setup         Before Test
Suite Teardown      After Test


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-building-entities-active.jsonld
${entity_building_filepath}=            building-simple-attributes.jsonld
${fragment_filename}=                   airQualityLevel-fragment.jsonld


*** Test Cases ***
046_13_01 Check That lastFailure And Status Are Updated If A Notification Could Not Be Sent
    [Documentation]    If the response to the notification request is different than 200 OK then implementations shall: Update notification.lastFailure with a timestamp representing the current date and time, update notification.status to "failed"
    [Tags]    sub-notification    5_8_6

    @{expected_notification_data_entities}=    Create List    Building
    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    Sleep    10s

    ${response}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}

    ${notification_info}=    Get Value From Json    ${response.json()}    $.notification

    Dictionary Should Contain Key    ${notification_info}[0]    status
    Should be Equal    failed    ${notification_info}[0][status]

    Dictionary Should Contain Key    ${notification_info}[0]    lastFailure
    ${last_failure_date}=    Parse Ngsild Date    ${notification_info}[0][lastFailure]
    Should Not Be Equal    ${last_failure_date}    ${None}


*** Keywords ***
Before Test
    Setup Initial Subscription
    Add Initial Entity

Setup Initial Subscription
    ${subscription_id}=    Generate Random Subscription Id
    ${create_response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}

Add Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${entity_id}

After Test
    Delete Initial Subscription
    Delete Initial Entity

Delete Initial Subscription
    Delete Subscription    ${subscription_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
