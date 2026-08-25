*** Settings ***
Documentation       Check that Subscription creation enforces the NotificationParams
...                 restrictions of Table 5.2.14.1-1 (CIM 009 V1.9.1, p.119-120):
...                 - "showChanges cannot be true in case format is keyValues" (and its
...                 declared synonym "simplified");
...                 - notification.attributes / pick / omit: "Empty array (0 length) is
...                 not allowed".
...                 Violations are BadRequestData per 5.8.1.4.
...
...                 Antares extension TP — the suite has no coverage for either rule.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
028_09_01 ShowChanges With KeyValues Format Is Rejected
    [Tags]    sub-create    5_2_14    5_8_1    since_v1.9.1
    Create Expecting 400    {"format": "keyValues", "showChanges": true}

028_09_02 ShowChanges With Simplified Format Is Rejected
    [Documentation]    "simplified" is Table 5.2.14.1-1's declared synonym of keyValues
    [Tags]    sub-create    5_2_14    5_8_1    since_v1.9.1
    Create Expecting 400    {"format": "simplified", "showChanges": true}

028_09_03 Empty Attributes Array Is Rejected
    [Tags]    sub-create    5_2_14    5_8_1    since_v1.9.1
    Create Expecting 400    {"attributes": []}

028_09_04 Empty Pick Array Is Rejected
    [Tags]    sub-create    5_2_14    5_8_1    since_v1.9.1
    Create Expecting 400    {"pick": []}

028_09_05 Empty Omit Array Is Rejected
    [Tags]    sub-create    5_2_14    5_8_1    since_v1.9.1
    Create Expecting 400    {"omit": []}

028_09_06 ShowChanges With Normalized Format Is Accepted
    [Documentation]    control: the restriction binds showChanges to keyValues only
    [Tags]    sub-create    5_2_14    5_8_1    since_v1.9.1
    ${id}=    Generate Random Subscription Id
    ${payload}=    Subscription Body    ${id}    {"format": "normalized", "showChanges": true}
    ${response}=    POST
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}
    ...    data=${payload}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    Delete Subscription    ${id}


*** Keywords ***
Subscription Body
    [Arguments]    ${id}    ${notification_extra}
    ${body}=    Evaluate
    ...    json.dumps({"id": $id, "type": "Subscription", "entities": [{"type": "Building"}], "notification": {"endpoint": {"uri": "http://localhost:18099/notify"}, **json.loads($notification_extra)}})
    ...    modules=json
    RETURN    ${body}

Create Expecting 400
    [Arguments]    ${notification_extra}
    ${id}=    Generate Random Subscription Id
    ${payload}=    Subscription Body    ${id}    ${notification_extra}
    ${response}=    POST
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}
    ...    data=${payload}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
