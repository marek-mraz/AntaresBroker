*** Settings ***
Documentation       Check that one can create a minimal context source registration subscription

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Generate Random Ids For Context Source Registration Subscriptions
Suite Teardown      Delete Created Context Source Registration Subscriptions


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription.jsonld


*** Test Cases ***
038_01_01 Create Context Source Registration Subscription
    [Documentation]    Check that one can create a minimal context source registration subscription
    [Tags]    csrsub-create    5_11_2
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}    ${subscription_id}
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${subscription_id}    ${response.headers}
    ${response1}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}

    # One needs to ignore the Additional Members ('lastFailure', 'lastNotification', 'timesFailed', 'timesSent')
    ${ignored_attributes}=    Create List
    ...    ${status_regex_expr}
    ...    ${lastfailure_regex_expr}
    ...    ${lastNotification_regex_expr}
    ...    ${timesFailed_regex_expr}
    ...    ${timesSent_regex_expr}

    Check Created Resource Set To    ${subscription_payload}    ${response1.json()}    ${ignored_attributes}

    Check Response Body Might Contain Additional Members of the NotificationParams
    ...    ${response1.json()}
    ...    lastNotification
    Check Response Body Might Contain Additional Members of the NotificationParams
    ...    ${response1.json()}
    ...    lastFailure
    Check Response Body Might Contain Additional Members of the NotificationParams
    ...    ${response1.json()}
    ...    lastSuccess
    Check Response Body Might Contain Additional Members of the NotificationParams    ${response1.json()}    timesSent


*** Keywords ***
Generate Random Ids For Context Source Registration Subscriptions
    ${subscription_id}=    Generate Random Subscription Id
    Set Suite Variable    ${subscription_id}

Delete Created Context Source Registration Subscriptions
    Delete Context Source Registration Subscription    ${subscription_id}
