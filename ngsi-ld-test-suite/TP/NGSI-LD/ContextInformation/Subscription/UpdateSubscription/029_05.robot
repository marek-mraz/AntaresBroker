*** Settings ***
Documentation       Check that one can update a subcription: Term to URI expansion of Attribute names shall be observed

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Subscriptions
Suite Teardown      Delete Initial Subscriptions


*** Variables ***
${subscription_payload_file_path}=                      subscriptions/subscription.jsonld
${subscription_update_fragment_file_path}=              subscriptions/fragments/subscription-vehicle-entities.json
${expected_subscription_payload_file_path}=             subscriptions/expectations/subscription-vehicle.jsonld
${expected_expanded_subscription_payload_file_path}=    subscriptions/expectations/subscription-vehicle-expanded-types.jsonld


*** Test Cases ***
029_05_01 Update Subscription With Term To Uri Expansion With Context
    [Documentation]    Check that one can update a subcription: Term to URI expansion of Attribute names shall be observed
    [Tags]    sub-update    5_8_2
    ${response}=    Update Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment_file_path}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    context=${ngsild_test_suite_context}
    ${ignore_keys}=    Create List    jsonldContext    timesFailed    timesSent    notificationTrigger
    Check Response Body Containing Subscription element
    ...    ${expected_subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${response1.json()}
    ...    ${ignore_keys}

029_05_02 Update Subscription With Term To Uri Expansion Without Context
    [Documentation]    Check that one can update a subcription: Term to URI expansion of Attribute names shall be observed
    [Tags]    sub-update    5_8_2
    ${response}=    Update Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment_file_path}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Subscription
    ...    id=${subscription_id}
    ${ignore_keys}=    Create List    jsonldContext    timesFailed    timesSent    notificationTrigger
    Check Response Body Containing Subscription element
    ...    ${expected_expanded_subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${response1.json()}
    ...    ${ignore_keys}


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
