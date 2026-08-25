*** Settings ***
Documentation       Check that one cannot retrieve a subscription if the subscription Id is not a valid URI, then an error of type BadRequestData shall be raised

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Retrieve Subscription With Invalid Id


*** Test Cases ***    ID
030_01_01 InvalidId
    [Tags]    sub-retrieve    5_8_3
    InvalidUri


*** Keywords ***
Retrieve Subscription With Invalid Id
    [Documentation]    Check that one cannot retrieve a subscription: If the subscription Id is not present or it is not a valid URI, then an error of type BadRequestData shall be raised
    [Arguments]    ${id}
    ${response}=    Retrieve Subscription
    ...    id=${id}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
