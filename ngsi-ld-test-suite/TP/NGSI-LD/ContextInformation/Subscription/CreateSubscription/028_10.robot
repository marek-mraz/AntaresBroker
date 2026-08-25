*** Settings ***
Documentation       Check the KeyValuePair data type (CIM 009 clause 5.2.22, Table 5.2.22-1)
...                 on the notification endpoint's receiverInfo/notifierInfo: both key and
...                 value are Strings with cardinality 1 — a pair with a non-string or
...                 missing key/value is BadRequestData.
...
...                 Antares extension TP — the official TPs only exercise string pairs.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
028_10_01 String Key And Value Pairs Are Accepted
    [Documentation]    control: Table 5.2.22-1-conforming pairs stay creatable
    [Tags]    sub-create    5_2_22    since_v1.9.1
    Create With Endpoint Extra Expecting    201
    ...    {"receiverInfo": [{"key": "Authorization", "value": "Bearer abc"}], "notifierInfo": [{"key": "Prefer", "value": "body=json"}]}

028_10_02 A Numeric ReceiverInfo Value Is Rejected
    [Tags]    sub-create    5_2_22    since_v1.9.1
    Create With Endpoint Extra Expecting    400    {"receiverInfo": [{"key": "K", "value": 42}]}

028_10_03 An Object NotifierInfo Value Is Rejected
    [Tags]    sub-create    5_2_22    since_v1.9.1
    Create With Endpoint Extra Expecting    400    {"notifierInfo": [{"key": "K", "value": {"a": 1}}]}

028_10_04 A Pair Without Value Is Rejected
    [Documentation]    value cardinality is 1
    [Tags]    sub-create    5_2_22    since_v1.9.1
    Create With Endpoint Extra Expecting    400    {"receiverInfo": [{"key": "K"}]}

028_10_05 A Numeric Key Is Rejected
    [Tags]    sub-create    5_2_22    since_v1.9.1
    Create With Endpoint Extra Expecting    400    {"receiverInfo": [{"key": 5, "value": "v"}]}


*** Keywords ***
Create With Endpoint Extra Expecting
    [Arguments]    ${expected_status_code}    ${endpoint_extra}
    ${id}=    Generate Random Subscription Id
    ${body}=    Evaluate
    ...    json.dumps({"id": $id, "type": "Subscription", "entities": [{"type": "Building"}], "notification": {"endpoint": {"uri": "http://localhost:18099/notify", **json.loads($endpoint_extra)}}})
    ...    modules=json
    ${response}=    POST
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}
    ...    data=${body}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    ${expected_status_code} == 201
        Delete Subscription    ${id}
    END
