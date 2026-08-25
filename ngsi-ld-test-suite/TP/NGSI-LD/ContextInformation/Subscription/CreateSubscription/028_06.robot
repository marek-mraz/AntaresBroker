*** Settings ***
Documentation       Check that one can create a subscription with a datasetId

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Teardown      Delete Initial Subscription


*** Variables ***
${subscription_payload_file_path}           subscriptions/subscription-building-with-datasetId.jsonld
${subscription_expectation_file_path}       subscriptions/expectations/subscription-with-datasetId.jsonld


*** Test Cases ***
028_06 Create A Subscription With A datasetId
    [Documentation]    Check that one can create a subscription with a datasetId
    [Tags]    sub-create    5_8_1    4_5_5    since_v1.8.1
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}
    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${expected_subscription}=    Load Test Sample    ${subscription_expectation_file_path}    ${subscription_id}
    ${response1}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    # Ignore the broker-synthesized jsonldContext output member (NGSI-LD 5.2.12), as sibling
    # create/retrieve tests (028_01, 030_03) already do; it is not part of this fixture's expectation.
    ${ignored_keys}=    Create List    jsonldContext
    Check Created Resource Set To    ${expected_subscription}    ${response1.json()}    ${ignored_keys}


*** Keywords ***
Delete Initial Subscription
    Delete Subscription    ${subscription_id}
