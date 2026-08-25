*** Settings ***
Documentation       Check that one can retrieve a subscription

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions
Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription.jsonld
${expectation_file_path}=               subscriptions/expectations/subscriptions-030-03.json


*** Test Cases ***
030_03_01 Retrieve Subscription
    [Documentation]    Check that one can retrieve a subscription
    [Tags]    sub-retrieve    5_8_3
    ${response}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${ignore_keys}=    Create List    jsonldContext    timesFailed    timesSent    notificationTrigger
    Check Response Body Containing Subscription element
    ...    ${expectation_file_path}
    ...    ${subscription_id}
    ...    ${response.json()}
    ...    ${ignore_keys}


*** Keywords ***
Setup Initial Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${create_response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}
