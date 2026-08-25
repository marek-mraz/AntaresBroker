*** Settings ***
Documentation       Check that one cannot retrieve a context source registration subscription with an invalid URI, an error of type BadRequestData shall be raised

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
040_02_01 Retrieve Context Source Registration Subscription With An Invalid Id
    [Documentation]    Check that one cannot retrieve a context source registration subscription with an invalid URI, an error of type BadRequestData shall be raised
    [Tags]    csrsub-retrieve    5_11_4
    ${response}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=invalidUri

    Check Response Status Code    400    ${response.status_code}
    Check Response Reason set to    ${response.reason}    Bad Request
    Check Response Headers Containing Content-Type set to    ${CONTENT_TYPE_JSON}    ${response.headers}
    Check Response Body Containing ProblemDetails Element
    ...    response_body=${response.json()}
    ...    problem_type=${ERROR_TYPE_BAD_REQUEST_DATA}
