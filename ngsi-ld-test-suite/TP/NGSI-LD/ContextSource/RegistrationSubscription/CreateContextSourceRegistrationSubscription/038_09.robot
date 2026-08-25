*** Settings ***
Documentation       Check that one cannot create a context source registration subscription with an expiration timestamp representing a moment before the current date and time

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription-expired.jsonld


*** Test Cases ***
038_09_01 Create Expired Context Source Registration Subscription
    [Documentation]    Check that one cannot create a context source registration subscription with an expiration timestamp representing a moment before the current date and time
    [Tags]    csrsub-create    5_11_2
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    response_body=${response.json()}
    ...    problem_type=${ERROR_TYPE_BAD_REQUEST_DATA}
