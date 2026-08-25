*** Settings ***
Documentation       Check that one cannot update a context source registration subscription with an invalid URI

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${subscription_update_fragment_file_path}=      csourceSubscriptions/fragments/subscription-update.json


*** Test Cases ***
039_02_01 Update Context Source Registration Subscription With Invalid Uri
    [Documentation]    Check that one cannot update a context source registration subscription with an invalid URI
    [Tags]    csrsub-update    5_11_3
    ${subscription_update_fragment}=    Load Test Sample    ${subscription_update_fragment_file_path}
    ${response}=    Update Context Source Registration Subscription    invalidUri    ${subscription_update_fragment}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    response_body=${response.json()}
    ...    problem_type=${ERROR_TYPE_BAD_REQUEST_DATA}
