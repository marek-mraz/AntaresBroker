*** Settings ***
Documentation       Verify the 5.2.12 Subscription value spaces the official
...                 028 TPs skip.
...
...                 Table 5.2.12-1: "Valid notification triggers are
...                 entityCreated, entityUpdated, entityDeleted,
...                 attributeCreated, attributeUpdated, attributeDeleted";
...                 csf is "A valid query string as per clause 4.9".
...                 Table 5.2.12-2: the read-only members "shall not be
...                 provided by Context Subscribers … implementations shall
...                 ignore them", and a retrieved Subscription is the 5.2.12
...                 data type — nothing else.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Library             Collections


*** Test Cases ***
5811_02_01 Invalid NotificationTrigger Is BadRequestData
    [Documentation]    Table 5.2.12-1: a notification trigger outside the six
    ...    valid values is not a Subscription the broker can honour → 400.
    [Tags]    sub-create    5_8_1    5_2_12    since_v1.9.1
    ${subscription_id}=    Generate Random Subscription Id
    ${payload}=    Evaluate
    ...    {"id": "${subscription_id}", "type": "Subscription", "entities": [{"type": "Building"}], "notificationTrigger": ["entityChanged"], "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}

5811_02_02 Valid NotificationTriggers Are Accepted
    [Documentation]    Table 5.2.12-1: the six listed triggers are the value
    ...    space — all of them together are a legal Subscription.
    [Tags]    sub-create    5_8_1    5_2_12    since_v1.9.1
    ${subscription_id}=    Generate Random Subscription Id
    ${payload}=    Evaluate
    ...    {"id": "${subscription_id}", "type": "Subscription", "entities": [{"type": "Building"}], "notificationTrigger": ["entityCreated", "entityUpdated", "entityDeleted", "attributeCreated", "attributeUpdated", "attributeDeleted"], "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    201    ${response.status_code}
    [Teardown]    Delete Subscription    ${subscription_id}

5811_02_03 Invalid Csf Is BadRequestData
    [Documentation]    Table 5.2.12-1: csf is "A valid query string as per
    ...    clause 4.9"; an unparseable filter would otherwise be stored and
    ...    silently match no Context Source Registration (5.11.2.4).
    [Tags]    sub-create    5_8_1    5_2_12    since_v1.9.1
    ${subscription_id}=    Generate Random Subscription Id
    ${payload}=    Evaluate
    ...    {"id": "${subscription_id}", "type": "Subscription", "entities": [{"type": "Building"}], "csf": "((", "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}

5811_02_04 A Retrieved Subscription Carries Only 5.2.12 Members
    [Documentation]    Table 5.2.12-2: read-only members provided by the
    ...    client are ignored, and the served representation exposes no
    ...    implementation-internal member — in particular the @context the
    ...    broker keeps for 5.8.6 notification rendering.
    [Tags]    sub-retrieve    5_8_3    5_2_12    since_v1.9.1
    ${subscription_id}=    Generate Random Subscription Id
    ${payload}=    Evaluate
    ...    {"id": "${subscription_id}", "type": "Subscription", "entities": [{"type": "Building"}], "__context": "http://localhost:1111/hijacked.jsonld", "status": "expired", "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}, "timesSent": 42}, "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Subscription From Subscription Payload    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Subscription
    ...    id=${subscription_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    __context
    Should Not Contain    ${response.text}    hijacked.jsonld
    Should Be Equal As Strings    ${response.json()['status']}    active
    Should Not Be Equal As Strings    ${response.json()['notification'].get('timesSent')}    42
    [Teardown]    Delete Subscription    ${subscription_id}
