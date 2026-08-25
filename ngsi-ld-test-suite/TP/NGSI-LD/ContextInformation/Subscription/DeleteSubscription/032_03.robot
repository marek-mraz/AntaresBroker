*** Settings ***
Documentation       Check that one can delete a subscription

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription.jsonld


*** Test Cases ***
032_03_01 Delete Subscription
    [Documentation]    Check that one can delete a subscription
    [Tags]    sub-delete    5_8_5
    ${response}=    Delete Subscription    ${subscription_id}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Subscription
    ...    id=${subscription_id}
    Check SUT Not Containing Resource    ${response1.status_code}


*** Keywords ***
Setup Initial Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${create_response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${subscription_id}
