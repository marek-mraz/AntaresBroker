*** Settings ***
Documentation       Check that one can query context source registration subscriptions with a limit parameter and it will be the maximum number of subscriptions to be retrieved

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration Subscriptions
Test Teardown       Delete Created Context Source Registration Subscriptions
Test Template       Query Context Source Registration Subscriptions With Limit Parameter


*** Variables ***
${first_subscription_payload_file_path}=        csourceSubscriptions/subscription.jsonld
${second_subscription_payload_file_path}=       csourceSubscriptions/subscription-watchedAttributes.jsonld
${third_subscription_payload_file_path}=        csourceSubscriptions/subscription-geoQ.jsonld
${expectation_file_path}=                       csourceSubscriptions/expectations/subscriptions-035-01.json


*** Test Cases ***    LIMIT    EXPECTED_SUBSCRIPTION_NUMBER
041_02_01 Query One Subscription
    [Tags]    csrsub-query    5_11_5
    ${1}    ${1}
041_02_02 Query Two Subscription
    [Tags]    csrsub-query    5_11_5
    ${2}    ${2}
041_02_03 Query All Subscriptions
    [Tags]    csrsub-query    5_11_5
    ${15}    ${3}


*** Keywords ***
Query Context Source Registration Subscriptions With Limit Parameter
    [Documentation]    Check that one can query context source registration subscriptions with a limit parameter and it will be the maximum number of subscriptions to be retrieved
    [Arguments]    ${limit}    ${expected_subscription_number}
    ${response}=    Query Context Source Registration Subscriptions
    ...    context=${ngsild_test_suite_context}
    ...    limit=${limit}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Number Of Entities
    ...    Subscription
    ...    ${expected_subscription_number}
    ...    ${response.json()}

Setup Initial Context Source Registration Subscriptions
    ${first_subscription_id}=    Generate Random Subscription Id
    ${second_subscription_id}=    Generate Random Subscription Id
    ${third_subscription_id}=    Generate Random Subscription Id
    ${first_subscription_payload}=    Load Test Sample
    ...    ${first_subscription_payload_file_path}
    ...    ${first_subscription_id}
    ${second_subscription_payload}=    Load Test Sample
    ...    ${second_subscription_payload_file_path}
    ...    ${second_subscription_id}
    ${third_subscription_payload}=    Load Test Sample
    ...    ${third_subscription_payload_file_path}
    ...    ${third_subscription_id}
    ${create_csrsub1_response}=    Create Context Source Registration Subscription    ${first_subscription_payload}
    Check Response Status Code    201    ${create_csrsub1_response.status_code}
    ${create_csrsub2_response}=    Create Context Source Registration Subscription    ${second_subscription_payload}
    Check Response Status Code    201    ${create_csrsub2_response.status_code}
    ${create_csrsub3_response}=    Create Context Source Registration Subscription    ${third_subscription_payload}
    Check Response Status Code    201    ${create_csrsub3_response.status_code}
    Set Suite Variable    ${first_subscription_id}
    Set Suite Variable    ${second_subscription_id}
    Set Test Variable    ${third_subscription_id}

Delete Created Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${first_subscription_id}
    Delete Context Source Registration Subscription    ${second_subscription_id}
    Delete Context Source Registration Subscription    ${third_subscription_id}
