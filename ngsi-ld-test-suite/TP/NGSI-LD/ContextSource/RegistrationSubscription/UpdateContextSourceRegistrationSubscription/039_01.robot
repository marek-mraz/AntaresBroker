*** Settings ***
Documentation       Check that one can update a context source registration subscription

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registration Subscriptions
Test Teardown       Delete Initial Context Source Registration Subscriptions


*** Variables ***
${subscription_payload_file_path}=              csourceSubscriptions/subscription.jsonld
${subscription_update_fragment_file_path}=      csourceSubscriptions/fragments/subscription-update.json


*** Test Cases ***
039_01_01 Update Context Source Registration Subscription
    [Documentation]    Check that one can update a context source registration subscription
    [Tags]    csrsub-update    5_11_3
    ${subscription_update_fragment}=    Load Test Sample    ${subscription_update_fragment_file_path}
    ${response}=    Update Context Source Registration Subscription
    ...    ${subscription_id}
    ...    ${subscription_update_fragment}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Does Not Contain Body    ${response}
    Check Response Reason set to    ${response.reason}    No Content

    ${subscription}=    Upsert Element In Entity    ${subscription_payload}    ${subscription_update_fragment}
    ${response1}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_keys}=    Create List    ${status_regex_expr}
    Check Updated Resource Set To
    ...    updated_resource=${subscription}
    ...    response_body=${response1.json()}
    ...    ignored_keys=${ignored_keys}


*** Keywords ***
Setup Initial Context Source Registration Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${create_csrsub_response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${create_csrsub_response.status_code}
    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${subscription_payload}

Delete Initial Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${subscription_id}
