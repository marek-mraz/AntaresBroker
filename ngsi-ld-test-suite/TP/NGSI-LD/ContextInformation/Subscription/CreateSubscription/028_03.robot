*** Settings ***
Documentation       Check that one cannot create a subscription with an invalid/empty id

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Teardown      Delete Initial Subscriptions
Test Template       Create Subscription With Invalid/Empty Id


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription.jsonld


*** Test Cases ***    ID
028_03_01 InvalidId
    invalidId
028_03_02 EmptyId
    ${EMPTY}


*** Keywords ***
Create Subscription With Invalid/Empty Id
    [Documentation]    Check that one cannot create a subscription with an invalid/empty id
    [Tags]    sub-create    5_8_1
    [Arguments]    ${subscription_id}
    Set Suite Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}
