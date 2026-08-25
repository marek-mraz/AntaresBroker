*** Settings ***
Documentation       Check that one cannot create a subscription with invalid pick / omit / attrs parameters

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Subscription With Invalid Pick Omit Attrs Parameters


*** Test Cases ***    FILE_NAME
028_08_01 Subscription With Same Entity Member In Pick And Omit
    [Tags]    sub-create    5_8_1    4_21    since_v1.8.1
    subscriptions/subscription-invalid-same-member-pick-omit.json
028_08_02 Subscription With Pick And Attrs
    [Tags]    sub-create    5_8_1    4_21    since_v1.8.1
    subscriptions/subscription-invalid-pick-and-attributes.json
028_08_03 Subscription With Omit And Attrs
    [Tags]    sub-create    5_8_1    4_21    since_v1.8.1
    subscriptions/subscription-invalid-omit-and-attributes.json


*** Keywords ***
Create Subscription With Invalid Pick Omit Attrs Parameters
    [Documentation]    Check that one cannot create a subscription with with invalid pick / omit / attrs parameters
    [Arguments]    ${file_name}
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${file_name}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
