*** Settings ***
Documentation       Check that one cannot delete a subscription: If the subscription id provided does not correspond to any existing subscription in the system then an error of type ResourceNotFound shall be raised

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
032_02_01 Delete Unknown Subscription
    [Documentation]    Check that one cannot delete a subscription: If the subscription id provided does not correspond to any existing subscription in the system then an error of type ResourceNotFound shall be raised
    [Tags]    sub-delete    5_8_5
    ${response}=    Delete Subscription    urn:ngsi-ld:Subscription:unknowSubscription
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
