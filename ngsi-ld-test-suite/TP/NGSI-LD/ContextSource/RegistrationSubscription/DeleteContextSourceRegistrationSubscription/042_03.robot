*** Settings ***
Documentation       Check that one cannot delete an unknown context source registration subscription

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
042_03_01 Delete Unknown Context Source Registration Subscription With Invalid Uri
    [Documentation]    Check that one cannot delete an unknown context source registration subscription
    [Tags]    csrsub-delete    5_11_6
    ${response}=    Delete Context Source Registration Subscription    urn:ngsi-ld:Subscription:unknowSubscription
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
