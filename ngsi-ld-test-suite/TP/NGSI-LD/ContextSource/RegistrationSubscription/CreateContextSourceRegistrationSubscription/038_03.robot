*** Settings ***
Documentation       Check that one can create a context source registration subscription without providing isActive member and will be active by default

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Generate Random Ids For Context Source Registration Subscriptions
Suite Teardown      Delete Created Context Source Registration Subscriptions


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription.jsonld


*** Test Cases ***
038_03_01 Create Context Source Registration Subscription Without isActive Member
    [Documentation]    Check that one can create a context source registration subscription without providing isActive member and will be active by default
    [Tags]    csrsub-create    5_11_2
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${subscription_id}    ${response.headers}
    ${response1}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=status
    ...    response_body=${response1.json()}
    ...    expected_attribute_value=active


*** Keywords ***
Generate Random Ids For Context Source Registration Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}

Delete Created Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${subscription_id}
