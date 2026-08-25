*** Settings ***
Documentation       Check that one cannot create a context source registration subscription where another context source registration subscription whose id is equivalent exists

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration Subscription
Test Teardown       Delete Initial Context Source Registration Subscription


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription.jsonld


*** Test Cases ***
038_07_01 Create Existing Context Source Registration Subscription
    [Documentation]    Check that one cannot create a context source registration subscription where another context source registration subscription whose id is equivalent exists
    [Tags]    csrsub-create    5_11_2
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    409    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    response_body=${response.json()}
    ...    problem_type=${ERROR_TYPE_ALREADY_EXISTS}


*** Keywords ***
Setup Initial Context Source Registration Subscription
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    Set Suite Variable    ${subscription_id}

Delete Initial Context Source Registration Subscription
    Delete Context Source Registration Subscription    ${subscription_id}
