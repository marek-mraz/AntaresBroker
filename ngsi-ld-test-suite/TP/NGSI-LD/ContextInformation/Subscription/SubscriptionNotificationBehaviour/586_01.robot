*** Settings ***
Documentation       5.8.6: a notification is sent once and booked once — notification.timesSent shall be incremented by one, lastNotification updated, and a failure updates lastFailure and status "failed". A delivery attempt the broker repeats on its own is transport, never a second notification: timesSent and timesFailed move by one per notification whatever the retry policy.

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
${second_fragment_filename}=            building-name-fragment.jsonld


*** Test Cases ***
586_01_01 A Failed Notification Is Booked Exactly Once
    [Documentation]    One change, one notification to an endpoint nobody listens on: timesSent 1, timesFailed 1, status failed, lastFailure set, lastNotification set, lastSuccess absent — whatever the broker's retry policy does afterwards.
    [Tags]    sub-notification    5_8_6

    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Sleep    10s

    ${response}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    ${notification_info}=    Get Value From Json    ${response.json()}    $.notification
    Should Be Equal    failed    ${notification_info}[0][status]
    Should Be Equal As Integers    1    ${notification_info}[0][timesSent]
    Should Be Equal As Integers    1    ${notification_info}[0][timesFailed]
    Dictionary Should Contain Key    ${notification_info}[0]    lastFailure
    Dictionary Should Contain Key    ${notification_info}[0]    lastNotification
    Dictionary Should Not Contain Key    ${notification_info}[0]    lastSuccess

586_01_02 Every Notification Is Booked Once More
    [Documentation]    A second change (a different attribute value) is a second notification: timesSent 2, timesFailed 2 — never more, so no attempt the broker repeated on its own was counted as a notification.
    [Tags]    sub-notification    5_8_6

    Update Entity Attributes    ${entity_id}    ${second_fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Sleep    10s

    ${response}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    ${notification_info}=    Get Value From Json    ${response.json()}    $.notification
    Should Be Equal As Integers    2    ${notification_info}[0][timesSent]
    Should Be Equal As Integers    2    ${notification_info}[0][timesFailed]
    Should Be Equal    failed    ${notification_info}[0][status]


*** Keywords ***
Before Test
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${entity_id}
    ${subscription_id}=    Generate Random Subscription Id
    ${create_response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}

After Test
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
