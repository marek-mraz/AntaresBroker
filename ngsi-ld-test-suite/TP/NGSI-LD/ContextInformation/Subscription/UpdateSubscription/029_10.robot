*** Settings ***
Documentation       Check that one can update a subscription: If only expiresAt is included and refers to a DateTime in the future, then status shall be updated to "active", if and only if the previous value of status was "expired"

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions
Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}               subscriptions/subscription.jsonld
${subscription_update_fragment_file_path}       subscriptions/fragments/subscription-expiresAt-future-update.json


*** Test Cases ***
029_10_01 Activate Expired Subscription
    [Documentation]    Check that one can update a subscription: If only expiresAt is included and refers to a DateTime in the future, then status shall be updated to "active", if and only if the previous value of status was "expired"
    [Tags]    sub-update    5_8_2
    # Update subscription to expire in 5 seconds
    ${now}=    Get Current Date    time_zone=UTC
    ${in_5_seconds}=    Add Time To Date    ${now}    5s    result_format=%Y-%m-%dT%H:%M:%SZ
    ${update_template_fragment}=    Load JSON From File
    ...    ${EXECDIR}/data/subscriptions/fragments/subscription-expiresAt-update.json
    ${update_fragment}=    Update Value To JSON    ${update_template_fragment}    $..expiresAt    ${in_5_seconds}
    ${update_response}=    Update Subscription With Payload
    ...    ${subscription_id}
    ...    ${update_fragment}
    ...    ${CONTENT_TYPE_JSON}
    Sleep    10s
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


*** Keywords ***
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
