*** Settings ***
Documentation       Check that one cannot create a subscription with invalid or unavailable jsonldContext

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Subscription With Invalid/Unavailable JsonldContext


*** Variables ***
${invalid_jsonldContext_subscription_file_path}=        subscriptions/subscription-building-with-invalid-jsonldContext.jsonld
${unavailable_jsonldContext_subscription_file_path}=    subscriptions/subscription-building-with-unavailable-jsonldContext.jsonld


*** Test Cases ***    FILE_NAME    EXPECTED_ERROR_TYPE    EXPECTED_STATUS_CODE
028_07_01 Subscription With Invalid jsonldContext
    [Tags]    sub-create    5_8_1    since_v1.7.1
    ${invalid_jsonldContext_subscription_file_path}    ${ERROR_TYPE_BAD_REQUEST_DATA}    400
028_07_02 Subscription With Unavailable jsonldContext
    [Tags]    sub-create    5_8_1    since_v1.7.1
    ${unavailable_jsonldContext_subscription_file_path}    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}    504


*** Keywords ***
Create Subscription With Invalid/Unavailable JsonldContext
    [Documentation]    Check that one cannot create a subscription with invalid or unavailable jsonldContext
    [Tags]    sub-create    5_8_1
    [Arguments]    ${file_name}    ${expected_error_type}    ${expected_status_code}
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${file_name}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${expected_error_type}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
