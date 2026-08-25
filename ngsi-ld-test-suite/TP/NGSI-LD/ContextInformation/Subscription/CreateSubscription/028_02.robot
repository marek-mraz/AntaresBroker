*** Settings ***
Documentation       Check that one cannot create a subscription with an invalid request

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Create Subscription With Invalid Request


*** Test Cases ***    FILENAME    EXPECTED_STATUS
028_02_01 InvalidJson
    subscription-invalid-json.jsonld    ${ERROR_TYPE_INVALID_REQUEST}
028_02_02 EmptyJson
    subscription-empty.jsonld    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Create Subscription With Invalid Request
    [Documentation]    Check that one cannot create a subscription with an invalid request
    [Tags]    sub-create    5_8_1
    [Arguments]    ${filename}    ${expected_status}
    ${response}=    Create Subscription From File    ${filename}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${expected_status}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
