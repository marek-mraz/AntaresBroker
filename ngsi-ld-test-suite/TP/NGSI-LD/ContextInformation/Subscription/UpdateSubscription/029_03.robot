*** Settings ***
Documentation       Check that one cannot update a subscription: If the data types and restriction are not met by the Subscription Fragment, then an error of type BadRequestData shall be raised

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions
Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}=              subscriptions/subscription.jsonld
${subscription_update_fragment_file_path}=      subscriptions/fragments/subscription-invalid-update.json


*** Test Cases ***
029_03_01 Update Subscription With Invalid Fragment
    [Documentation]    Check that one cannot update a subscription: If the data types and restriction are not met by the Subscription Fragment, then an error of type BadRequestData shall be raised
    [Tags]    sub-update    5_8_2
    ${response}=    Update Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment_file_path}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


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
