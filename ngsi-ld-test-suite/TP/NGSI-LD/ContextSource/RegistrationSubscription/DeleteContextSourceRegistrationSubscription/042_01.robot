*** Settings ***
Documentation       Check that one can delete a context source registration subscription

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration Subscription


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription.jsonld


*** Test Cases ***
042_01_01 Delete Context Source Registration Subscription
    [Documentation]    Check that one can delete a context source registration subscription
    [Tags]    csrsub-delete    5_11_6
    ${response}=    Delete Context Source Registration Subscription    ${subscription_id}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}
    ...    context=${ngsild_test_suite_context}
    Check SUT Not Containing Resource    ${response1.status_code}


*** Keywords ***
Setup Initial Context Source Registration Subscription
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    Set Suite Variable    ${subscription_id}
