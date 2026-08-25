*** Settings ***
Documentation       Check that one can query a list of subscriptions

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions
Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${first_subscription_payload_file_path}=        subscriptions/subscription.jsonld
${second_subscription_payload_file_path}=       subscriptions/subscription-watchedAttributes.jsonld
${third_subscription_payload_file_path}=        subscriptions/subscription-inactive.jsonld
${expectation_file_path}=                       subscriptions/expectations/subscriptions-031-01.json


*** Test Cases ***
031_01_01 Query Subscriptions
    [Documentation]    Check that one can query a list of subscriptions
    [Tags]    sub-query    5_8_4
    ${response}=    Query Subscriptions    context=${ngsild_test_suite_context}
    @{subscription_ids}=    Create List
    ...    ${first_subscription_id}
    ...    ${second_subscription_id}
    ...    ${third_subscription_id}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Subscription elements
    ...    ${expectation_file_path}
    ...    ${subscription_ids}
    ...    ${response.json()}


*** Keywords ***
Setup Initial Subscriptions
    ${first_subscription_id}=    Generate Random Subscription Id
    ${create_response1}=    Create Subscription
    ...    ${first_subscription_id}
    ...    ${first_subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Suite Variable    ${first_subscription_id}
    ${second_subscription_id}=    Generate Random Subscription Id
    ${create_response2}=    Create Subscription
    ...    ${second_subscription_id}
    ...    ${second_subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Suite Variable    ${second_subscription_id}
    ${third_subscription_id}=    Generate Random Subscription Id
    ${create_response3}=    Create Subscription
    ...    ${third_subscription_id}
    ...    ${third_subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}
    Set Suite Variable    ${third_subscription_id}

Delete Initial Subscriptions
    Delete Subscription    ${first_subscription_id}
    Delete Subscription    ${second_subscription_id}
    Delete Subscription    ${third_subscription_id}
