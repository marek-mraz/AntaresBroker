*** Settings ***
Documentation       Check that one cannot delete a context source registration subscription with an invalid URI

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
042_02_01 Delete Context Source Registration Subscription With Invalid Uri
    [Documentation]    Check that one cannot delete a context source registration subscription with an invalid URI
    [Tags]    csrsub-delete    5_11_6
    ${response}=    Delete Context Source Registration Subscription    invalidUri
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
