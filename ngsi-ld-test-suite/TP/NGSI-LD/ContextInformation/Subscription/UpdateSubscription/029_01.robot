*** Settings ***
Documentation       Check that one cannot update a subscription: If the Subscription id is not present or it is not a valid URI, then an error of type BadRequestData shall be raised

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Update Subscription With Non present Or Invalid Id


*** Variables ***
${subscription_update_fragment_file_path}=      subscriptions/fragments/subscription-update.json


*** Test Cases ***    ID    EXPECTED_STATUS_CODE    PROBLEM_TYPE
029_01_01 NotPresentId
    [Tags]    sub-update    5_8_2
    ${EMPTY}    405    ${EMPTY}
029_01_02 InvalidId
    [Tags]    sub-update    5_8_2
    InvalidUri    400    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Update Subscription With Non present Or Invalid Id
    [Documentation]    Check that one cannot update a subscription: If the Subscription id is not present or it is not a valid URI, then an error of type BadRequestData shall be raised
    [Arguments]    ${id}    ${expected_status_code}    ${problem_type}
    ${response}=    Update Subscription    ${id}    ${subscription_update_fragment_file_path}    ${CONTENT_TYPE_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    "${problem_type}"!="${EMPTY}"
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${problem_type}
        Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    END
