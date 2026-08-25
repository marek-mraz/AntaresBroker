*** Settings ***
Documentation       Check that one cannot create a subscription with invalid throttling

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Create Subscription With Invalid Throttling


*** Test Cases ***    FILENAME    EXPECTED_STATUS
028_05_01 ThrottlingAndTimeIntervalConjunction
    subscription-invalid-throttling-timeInterval.jsonld    ${ERROR_TYPE_BAD_REQUEST_DATA}
028_05_02 NegativeThrottling
    subscription-invalid-negative-throttling.jsonld    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Create Subscription With Invalid Throttling
    [Documentation]    Check that one cannot create a subscription with invalid throttling
    [Tags]    sub-create    5_8_1
    [Arguments]    ${filename}    ${expected_status}
    ${response}=    Create Subscription From File    ${filename}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${expected_status}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
