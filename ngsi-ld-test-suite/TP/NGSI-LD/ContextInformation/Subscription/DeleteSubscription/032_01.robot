*** Settings ***
Documentation       Check that one cannot delete a subscription: If the subscription Id is not present or it is not a valid URI, then an error shall be raised

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Delete Subscription With Non present Or Invalid Id


*** Test Cases ***    ID    EXPECTED_STATUS_CODE    PROBLEM_TYPE
032_01_01 NotPresentId
    [Tags]    sub-delete    5_8_5
    ${EMPTY}    405    ${EMPTY}
032_01_02 InvalidId
    [Tags]    sub-delete    5_8_5
    InvalidUri    400    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Delete Subscription With Non present Or Invalid Id
    [Documentation]    Check that one cannot delete a subscription: If the subscription Id is not present or it is not a valid URI, then an error shall be raised
    [Arguments]    ${id}    ${expected_status_code}    ${problem_type}
    ${response}=    Delete Subscription    ${id}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    "${problem_type}"!="${EMPTY}"
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${problem_type}
        Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    END
