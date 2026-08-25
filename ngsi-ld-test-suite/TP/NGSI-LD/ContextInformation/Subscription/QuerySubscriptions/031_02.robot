*** Settings ***
Documentation       Check that one can query a list of subscriptions: Pagination logic shall be in place

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Subscriptions
Test Teardown       Delete Initial Subscriptions
Test Template       Query Subscriptions With Limit And Page Parameters


*** Variables ***
${first_subscription_payload_file_path}=        subscriptions/subscription.jsonld
${second_subscription_payload_file_path}=       subscriptions/subscription-watchedAttributes.jsonld
${third_subscription_payload_file_path}=        subscriptions/subscription-inactive.jsonld


*** Test Cases ***    LIMIT    OFFSET    EXPECTED_SUBSCRIPTION_NUMBER    PREV_LINK    NEXT_LINK
031_02_01 Query Second Subscription
    [Tags]    sub-query    5_8_4
    ${1}    ${1}    ${1}    </ngsi-ld/v1/subscriptions?limit=1&offset=0>;rel="prev";type="application/ld+json"    </ngsi-ld/v1/subscriptions?limit=1&offset=2>;rel="next";type="application/ld+json"
031_02_02 Query Last Subscription
    [Tags]    sub-query    5_8_4
    ${1}    ${2}    ${1}    </ngsi-ld/v1/subscriptions?limit=1&offset=1>;rel="prev";type="application/ld+json"    ${EMPTY}
031_02_03 Query All Subscriptions
    [Tags]    sub-query    5_8_4
    ${15}    ${0}    ${3}    ${EMPTY}    ${EMPTY}


*** Keywords ***
Query Subscriptions With Limit And Page Parameters
    [Documentation]    Check that one can query a list of subscriptions: Pagination logic shall be in place
    [Arguments]    ${limit}    ${offset}    ${expectation_subscription_number}    ${prev_link}    ${next_link}
    ${response}=    Query Subscriptions
    ...    context=${ngsild_test_suite_context}
    ...    limit=${limit}
    ...    offset=${offset}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Number Of Entities
    ...    Subscription
    ...    ${expectation_subscription_number}
    ...    ${response.json()}
    Check Pagination Prev And Next Headers    ${prev_link}    ${next_link}    ${response.headers}

Setup Initial Subscriptions
    ${first_subscription_id}=    Generate Random Subscription Id
    ${create_response1}=    Create Subscription
    ...    ${first_subscription_id}
    ...    ${first_subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_subscription_id}
    ${second_subscription_id}=    Generate Random Subscription Id
    ${create_response2}=    Create Subscription
    ...    ${second_subscription_id}
    ...    ${second_subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_subscription_id}
    ${third_subscription_id}=    Generate Random Subscription Id
    ${create_response3}=    Create Subscription
    ...    ${third_subscription_id}
    ...    ${third_subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}
    Set Test Variable    ${third_subscription_id}

Delete Initial Subscriptions
    Delete Subscription    ${first_subscription_id}
    Delete Subscription    ${second_subscription_id}
    Delete Subscription    ${third_subscription_id}
