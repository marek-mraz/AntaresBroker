*** Settings ***
Documentation       Check that one can create a context source registration subscription with an expiresAt member and when it is due the status of the subscription changes to "expired"

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Generate Random Ids For Context Source Registration Subscriptions
Suite Teardown      Delete Created Context Source Registration Subscriptions


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription-expiresAt.jsonld
${date_format}=                         %Y-%m-%dT%H:%M:%SZ


*** Test Cases ***
038_05_01 Create Context Source Registration Subscription With expiresAt Member
    [Documentation]    Check that one can create a context source registration subscription with an expiresAt member and when it is due the status of the subscription changes to "expired"
    [Tags]    csrsub-create    5_11_2
    ${subscription_payload_sample}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${current_date}=    Get Current Date    time_zone=UTC    result_format=${date_format}
    ${expiresAt}=    Add Time To Date    ${current_date}    10 seconds    result_format=${date_format}
    ${subscription_payload}=    Update Value To JSON    ${subscription_payload_sample}    $..expiresAt    ${expiresAt}
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${subscription_id}    ${response.headers}
    Sleep    15s

    ${response1}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}

    Check Response Body Contains DateTime Value
    ...    dictionary=${response1.json()}
    ...    key=expiresAt
    ...    expected value=${expiresAt}

    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=status
    ...    response_body=${response1.json()}
    ...    expected_attribute_value=expired


*** Keywords ***
Generate Random Ids For Context Source Registration Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}

Delete Created Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${subscription_id}
