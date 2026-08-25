*** Settings ***
Documentation       Check that one can create a context source registration subscription with isActive member set to false and it's initial status will be set to "paused"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Generate Random Ids For Context Source Registration Subscriptions
Suite Teardown      Delete Created Context Source Registration Subscriptions


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription-inactive.jsonld


*** Test Cases ***
038_04_01 Create Inactive Context Source Registration Subscription
    [Documentation]    Check that one can create a context source registration subscription with isActive member set to false and it's initial status will be set to "paused"
    [Tags]    csrsub-create    5_11_2
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${subscription_id}    ${response.headers}
    ${response1}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}

    # The conversion of a boolean data to json for a python object transform false -> False and true -> True
    Check Response Body Containing a Boolean Attribute set to
    ...    expected_attribute_name=isActive
    ...    response_body=${response1.json()}
    ...    expected_attribute_value=False

    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=status
    ...    response_body=${response1.json()}
    ...    expected_attribute_value=paused


*** Keywords ***
Generate Random Ids For Context Source Registration Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}

Delete Created Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${subscription_id}
