*** Settings ***
Documentation       Check that one cannot create a subscription with an existing id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Subscription
Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription.jsonld


*** Test Cases ***
028_04_01 Create A Subscription With An Id Known To The System
    [Documentation]    Check that one cannot create a subscription with an existing id
    [Tags]    sub-create    5_8_1
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    409    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_ALREADY_EXISTS}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Create Initial Subscription
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${initial_response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${initial_response.status_code}

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}
