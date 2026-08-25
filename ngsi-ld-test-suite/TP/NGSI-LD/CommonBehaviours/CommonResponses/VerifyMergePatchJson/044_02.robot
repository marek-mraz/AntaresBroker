*** Settings ***
Documentation       Verify that PATCH HTTP requests can be done with "application/merge-patch+json" as Content-Type

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Subscription
Test Teardown       Delete Initial Subscription


*** Variables ***
${subscription_filename}=       subscriptions/subscription.jsonld
${subscription_fragment}=       subscriptions/fragments/subscription-update.json


*** Test Cases ***
044_02_01 Endpoint /subscriptions/{subscriptionId}
    [Documentation]    Verify that PATCH HTTP requests can be done with "application/merge-patch+json" as Content-Type
    [Tags]    sub-update    cb-mergepatch    6_3_4
    ${response}=    Update Subscription
    ...    ${subscription_id}
    ...    ${subscription_fragment}
    ...    ${CONTENT_TYPE_MERGE_PATCH_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}


*** Keywords ***
Create Initial Subscription
    ${subscription_id}=    Generate Random Subscription Id
    ${response}=    Create Subscription    ${subscription_id}    ${subscription_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${subscription_id}

Delete Initial Subscription
    Delete Subscription    ${subscription_id}
