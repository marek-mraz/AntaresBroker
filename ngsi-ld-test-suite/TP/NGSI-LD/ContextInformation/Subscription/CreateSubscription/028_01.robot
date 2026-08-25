*** Settings ***
Documentation       Check that one can create a subscription

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}=          subscriptions/subscription.jsonld
${subscription_expectation_file_path}=      subscriptions/expectations/subscription-028-01.jsonld


*** Test Cases ***
028_01_01 Create Subscription
    [Documentation]    Check that one can create a subscription
    [Tags]    sub-create    5_8_1
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    ${expected_subscription}=    Load Test Sample    ${subscription_expectation_file_path}    ${subscription_id}
    ${response1}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    ${ignore_keys}=    Create List    jsonldContext    timesFailed    timesSent    notificationTrigger
    Check Created Resource Set To    ${expected_subscription}    ${response1.json()}    ${ignore_keys}


*** Keywords ***
Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}
