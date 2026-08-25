*** Settings ***
Documentation       Check that one can query context source registration subscriptions

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration Subscriptions
Test Teardown       Delete Created Context Source Registration Subscriptions


*** Variables ***
${first_subscription_payload_file_path}=        csourceSubscriptions/subscription.jsonld
${second_subscription_payload_file_path}=       csourceSubscriptions/subscription-watchedAttributes.jsonld
${expectation_file_path}=                       csourceSubscriptions/expectations/subscriptions-035-01.json


*** Test Cases ***
041_01_01 Query Context Source Registration Subscriptions
    [Documentation]    Check that one can query context source registration subscriptions
    [Tags]    csrsub-query    5_11_5
    ${response}=    Query Context Source Registration Subscriptions    context=${ngsild_test_suite_context}
    @{subscription_ids}=    Create List    ${first_subscription_id}    ${second_subscription_id}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Subscription elements
    ...    ${expectation_file_path}
    ...    ${subscription_ids}
    ...    ${response.json()}


*** Keywords ***
Setup Initial Context Source Registration Subscriptions
    ${first_subscription_id}=    Generate Random Subscription Id
    ${second_subscription_id}=    Generate Random Subscription Id
    ${first_subscription_payload}=    Load Test Sample
    ...    ${first_subscription_payload_file_path}
    ...    ${first_subscription_id}
    ${second_subscription_payload}=    Load Test Sample
    ...    ${second_subscription_payload_file_path}
    ...    ${second_subscription_id}
    ${create_csrsub1_response}=    Create Context Source Registration Subscription    ${first_subscription_payload}
    Check Response Status Code    201    ${create_csrsub1_response.status_code}
    ${create_csrsub2_response}=    Create Context Source Registration Subscription    ${second_subscription_payload}
    Check Response Status Code    201    ${create_csrsub2_response.status_code}
    Set Suite Variable    ${first_subscription_id}
    Set Suite Variable    ${second_subscription_id}

Delete Created Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${first_subscription_id}
    Delete Context Source Registration Subscription    ${second_subscription_id}
