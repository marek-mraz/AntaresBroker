*** Settings ***
Documentation       Check that one can create a context source registration subscription without providing an id and it will be automatically generated

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistrationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Teardown      Delete Created Context Source Registration Subscriptions


*** Variables ***
${subscription_payload_file_path}=      csourceSubscriptions/subscription.jsonld
${subscription_id}=                     ${EMPTY}


*** Test Cases ***
038_02_01 Create Context Source Registration Subscription Without An Id
    [Documentation]    Check that one can create a context source registration subscription without providing an id and it will be automatically generated
    [Tags]    csrsub-create    5_11_2
    ${subscription_payload}=    Load Test Sample    ${subscription_payload_file_path}
    Remove From Dictionary    ${subscription_payload}    id
    ${response}=    Create Context Source Registration Subscription    ${subscription_payload}

    Dictionary Should Contain Key    ${response.headers}    Location    msg=HTTP Headers do not contain key 'Location'
    ${subscription_id}=    Check Response Headers ID Not Empty    ${response.headers}
    Set Suite Variable    ${subscription_id}

    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${response1}=    Retrieve Context Source Registration Subscription
    ...    subscription_id=${subscription_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_attributes}=    Create List    ${id_regex_expr}    ${status_regex_expr}
    Check Created Resource Set To    ${subscription_payload}    ${response1.json()}    ${ignored_attributes}


*** Keywords ***
Delete Created Context Source Registration Subscriptions
    IF    "${subscription_id}" != "${EMPTY}"
        Delete Context Source Registration Subscription    ${subscription_id}
    ELSE
        Log To Console    \nThere was no Context Source Registration Subscription to delete\n
    END
