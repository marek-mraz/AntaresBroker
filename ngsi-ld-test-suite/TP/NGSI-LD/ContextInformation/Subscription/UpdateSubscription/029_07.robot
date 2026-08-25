*** Settings ***
Documentation       Check that one can update a subscription: If isActive is equal to true and expiresAt is not present, then status shall be updated to "active", if and only if, the previous value of status was different than "expired"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions
Suite Teardown      Delete Initial Subscriptions
Test Template       Activate Paused Subscription With isActive Member


*** Variables ***
${subscription_payload_file_path}=      subscriptions/subscription-inactive.jsonld


*** Test Cases ***    SUBSCRIPTION_UPDATE_FRAGMENT_FILE_PATH
029_07_01 ActiveTrue
    [Tags]    sub-update    5_8_2
    subscriptions/fragments/subscription-isActive-true-update.json


*** Keywords ***
Activate Paused Subscription With isActive Member
    [Documentation]    Check that one can update a subscription: If isActive is equal to true and expiresAt is not present, then status shall be updated to "active", if and only if, the previous value of status was different than "expired"
    [Arguments]    ${subscription_update_fragment_file_path}
    ${response}=    Update Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment_file_path}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Subscription
    ...    id=${subscription_id}
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=status
    ...    response_body=${response1.json()}
    ...    expected_attribute_value=active

Setup Initial Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${initial_response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${initial_response.status_code}
    Set Suite Variable    ${subscription_id}

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}
