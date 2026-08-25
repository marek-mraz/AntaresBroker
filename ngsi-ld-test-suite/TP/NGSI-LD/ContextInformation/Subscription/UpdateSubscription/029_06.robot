*** Settings ***
Documentation       Check that one can update a subscription: The implementation shall modify the target Subscription

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions
Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}=              subscriptions/subscription.jsonld
${subscription_update_fragment_file_path}=      subscriptions/fragments/subscription-update.json


*** Test Cases ***
029_06_01 Update Subscription
    [Documentation]    Check that one can update a subscription: The implementation shall modify the target Subscription
    [Tags]    sub-update    5_8_2
    ${response}=    Update Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment_file_path}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${subscription_update_fragment}=    Load Test Sample    ${subscription_update_fragment_file_path}
    ${subscription}=    Upsert Element In Entity    ${subscription_payload}    ${subscription_update_fragment}
    ${response1}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    ${ignored_attributes}=    Create List
    ...    ${status_regex_expr}
    ...    jsonldContext
    ...    timesFailed
    ...    timesSent
    ...    notificationTrigger
    Check Updated Resource Set To
    ...    updated_resource=${subscription}
    ...    response_body=${response1.json()}
    ...    ignored_keys=${ignored_attributes}


*** Keywords ***
Setup Initial Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${initial_response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${initial_response.status_code}
    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${subscription_payload}

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}
