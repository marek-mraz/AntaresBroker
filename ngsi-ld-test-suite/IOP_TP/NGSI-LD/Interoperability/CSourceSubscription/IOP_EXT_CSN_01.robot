*** Settings ***
Documentation       Context Source Registration Subscriptions & notifications
...                 (Antares extension IOP TPs). 5.11.2.4/5.11.7: "initially
...                 on subscription and whenever there is a change of a
...                 matching Context Source Registration (creation, update,
...                 deletion), implementations shall post a new
...                 CsourceNotification ... providing the Context Source
...                 Registration(s) together with the appropriate trigger
...                 reason in the triggerReason member". 5.11.3.4: on update
...                 "send a notification with all currently matching Context
...                 Source Registrations". 5.11.6.4: delete → "no longer
...                 perform notifications concerning that Subscription".
...                 5.3.2 Table 5.3.2-1: id, type=ContextSourceNotification,
...                 data (CSourceRegistration[]), notifiedAt, subscriptionId,
...                 triggerReason — all cardinality 1.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary
Library             HttpCtrl.Server

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}
${notify_host}      127.0.0.1
${notify_port}      8094


*** Test Cases ***
IOP_EXT_CSN_01_01 Creating A Matching Registration Notifies With Its Body
    [Documentation]    5.11.2.4: "whenever there is a change of a matching
    ...    Context Source Registration (creation, update, deletion),
    ...    implementations shall post a new CsourceNotification" — the
    ...    created registration travels in data with triggerReason
    ...    newlyMatching (5.3.3).
    [Tags]    iop    iop-ext    5_11_2    5_3_3    since_v1.9.1
    Start Notify Server
    Post CSource Subscription    ${etype}
    Drain Initial Notification

    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    ${doc}=    Evaluate    __import__('json').loads($body.decode('utf-8'))
    Should Be Equal    ${doc['triggerReason']}    newlyMatching
    Should Be Equal    ${doc['data'][0]['id']}    ${registration_id}
    Should Not Contain    ${body.decode('utf-8')}    ${registration_id}-2

IOP_EXT_CSN_01_02 The Initial Notification Carries All Matching Registrations
    [Documentation]    5.11.7: the csource notification "shall be sent on
    ...    initial subscription" — with two matching registrations already
    ...    present, both appear; a differently-typed one does not.
    [Tags]    iop    iop-ext    5_11_2    5_11_7    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b2_url}    Other${etype}
    Start Notify Server
    Post CSource Subscription    ${etype}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    ${text}=    Evaluate    $body.decode('utf-8')
    Should Contain    ${text}    ${registration_id}
    Should Contain    ${text}    ${registration_id}-2
    Should Not Contain    ${text}    ${registration_id}-3

IOP_EXT_CSN_01_03 A Registration Update Notifies With triggerReason updated
    [Documentation]    5.11.7: a change "triggered by ... the update of a
    ...    csource registration (whether matching before the update, after
    ...    the update or in both cases)" notifies with "the appropriate
    ...    trigger reason".
    [Tags]    iop    iop-ext    5_11_7    5_3_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    Post CSource Subscription    ${etype}
    Drain Initial Notification

    ${fragment}=    Evaluate    {"description": "updated by IOP_EXT_CSN_01_03"}
    ${response}=    Patch Registration At Broker    ${b1_url}    ${registration_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    ${doc}=    Evaluate    __import__('json').loads($body.decode('utf-8'))
    Should Be Equal    ${doc['triggerReason']}    updated
    Should Contain    ${body.decode('utf-8')}    ${registration_id}
    Should Not Be Equal    ${doc['triggerReason']}    noLongerMatching

IOP_EXT_CSN_01_04 Deletion Notifies Once; Unrelated Churn Stays Silent
    [Documentation]    5.11.7: deletion of a matching registration notifies
    ...    (noLongerMatching, 5.3.3 "because it was deleted"); a subsequent
    ...    non-matching registration creation produces nothing.
    [Tags]    iop    iop-ext    5_11_7    5_3_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    Post CSource Subscription    ${etype}
    Drain Initial Notification

    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    ${doc}=    Evaluate    __import__('json').loads($body.decode('utf-8'))
    Should Be Equal    ${doc['triggerReason']}    noLongerMatching
    Should Contain    ${body.decode('utf-8')}    ${registration_id}

    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b2_url}    Other${etype}
    Wait For No Request    ${3}

IOP_EXT_CSN_01_05 The Entities Filter Gates CSource Notifications
    [Documentation]    5.11.2.4: "the entities specified in the subscription
    ...    ... are matched against the respective information property of
    ...    the Context Source registrations" — a registration for another
    ...    type never notifies.
    [Tags]    iop    iop-ext    5_11_2    since_v1.9.1
    Start Notify Server
    Post CSource Subscription    ${etype}
    Drain Initial Notification

    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b2_url}    Other${etype}
    Sleep    1s
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${registration_id}
    Should Not Contain    ${body.decode('utf-8')}    ${registration_id}-3

IOP_EXT_CSN_01_06 CsourceNotification Members Are Exactly The 5.3.2 Set
    [Documentation]    5.3.2 Table 5.3.2-1: id, type equal to
    ...    "ContextSourceNotification", data (CSourceRegistration[]),
    ...    notifiedAt, subscriptionId, triggerReason — and nothing else.
    [Tags]    iop    iop-ext    5_3_2    since_v1.9.1
    Start Notify Server
    Post CSource Subscription    ${etype}
    Drain Initial Notification
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    ${doc}=    Evaluate    __import__('json').loads($body.decode('utf-8'))
    Should Be Equal    ${doc['type']}    ContextSourceNotification
    Should Be Equal    ${doc['subscriptionId']}    ${subscription_id}
    Dictionary Should Contain Key    ${doc}    notifiedAt
    Dictionary Should Contain Key    ${doc}    id
    Should Be Equal    ${doc['data'][0]['type']}    ContextSourceRegistration
    ${extra}=    Evaluate
    ...    [k for k in $doc if k not in ("id", "type", "data", "notifiedAt", "subscriptionId", "triggerReason", "@context")]
    Should Be Empty    ${extra}

IOP_EXT_CSN_01_07 Updating The Watched Type Re-Aims The Notifications
    [Documentation]    5.11.3.4: after the subscription update
    ...    implementations "send a notification with all currently matching
    ...    Context Source Registrations" — the newly watched type's
    ...    registration appears, the old type's does not.
    [Tags]    iop    iop-ext    5_11_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-2    ${b3_url}    Other${etype}
    Start Notify Server
    Post CSource Subscription    ${etype}
    Drain Initial Notification

    ${fragment}=    Evaluate    {"entities": [{"type": "Other" + $etype}]}
    ${response}=    Patch CSource Subscription    ${subscription_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${registration_id}-2
    Should Not Contain    ${body.decode('utf-8')}    "${registration_id}"

IOP_EXT_CSN_01_08 CSource Subscriptions Are Queryable; Unknown Ids Are 404
    [Documentation]    5.11.4.4/5.11.5.4: the stored subscriptions are
    ...    listed and retrievable; "If the identifier provided does not
    ...    correspond to any existing subscription in the system then an
    ...    error of type ResourceNotFound shall be raised."
    [Tags]    iop    iop-ext    5_11_4    5_11_5    since_v1.9.1
    Start Notify Server
    Post CSource Subscription    ${etype}

    ${listed}=    GET    url=${b1_url}/csourceSubscriptions    expected_status=any
    Check Response Status Code    200    ${listed.status_code}
    Should Contain    ${listed.text}    ${subscription_id}
    ${one}=    GET    url=${b1_url}/csourceSubscriptions/${subscription_id}    expected_status=any
    Check Response Status Code    200    ${one.status_code}
    Should Be Equal    ${one.json()['id']}    ${subscription_id}
    ${missing}=    GET    url=${b1_url}/csourceSubscriptions/urn:ngsi-ld:Subscription:absent-${suffix}
    ...    expected_status=any
    Check Response Status Code    404    ${missing.status_code}
    Should Contain    ${missing.text}    ResourceNotFound


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopCsn${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopCsn:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopcsn-${suffix}
    Set Test Variable    ${subscription_id}    urn:ngsi-ld:Subscription:iopcsn-${suffix}
    Set Test Variable    ${server_started}    ${False}

Start Notify Server
    Start Server    ${notify_host}    ${notify_port}
    Set Test Variable    ${server_started}    ${True}

Post CSource Subscription
    [Arguments]    ${type}
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $type}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/csn", "accept": "application/json"}}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${b1_url}/csourceSubscriptions    json=${sub}    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Patch CSource Subscription
    [Arguments]    ${sid}    ${fragment}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    PATCH    url=${b1_url}/csourceSubscriptions/${sid}    json=${fragment}
    ...    headers=${headers}    expected_status=any
    RETURN    ${response}

Drain Initial Notification
    [Documentation]    5.11.7: a csource notification is sent "on initial
    ...    subscription" — absorb it (or its absence when nothing matches)
    ...    so the test's own assertions see only the triggered one.
    ${status}    ${_}=    Run Keyword And Ignore Error    Wait For Request    ${5}
    IF    '${status}' == 'PASS'
        Reply By    200
    END

Cleanup Interop Fixtures
    ${sid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($subscription_id, safe='')
    DELETE    url=${b1_url}/csourceSubscriptions/${sid}    expected_status=any
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-2
    Delete Registration At Broker    ${b1_url}    ${registration_id}-3
    IF    ${server_started}
        Stop Server
    END
