*** Settings ***
Documentation       Check that one cannot update an unknown context source registration subscription

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${subscription_update_fragment_file_path}=      csourceSubscriptions/fragments/subscription-update.json


*** Test Cases ***
039_03_01 Update Unknown Context Source Registration Subscription
    [Documentation]    Check that one cannot update an unknown context source registration subscription
    [Tags]    csrsub-update    5_11_3
    ${subscription_update_fragment}=    Load Test Sample    ${subscription_update_fragment_file_path}
    ${response}=    Update Context Source Registration Subscription
    ...    urn:ngsi-ld:Subscription:unknowSubscription
    ...    ${subscription_update_fragment}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    response_body=${response.json()}
    ...    problem_type=${ERROR_TYPE_RESOURCE_NOT_FOUND}
