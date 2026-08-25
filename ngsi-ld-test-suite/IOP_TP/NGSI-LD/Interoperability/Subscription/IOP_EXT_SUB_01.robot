*** Settings ***
Documentation       Distributed subscriptions I (Antares extension IOP TPs).
...                 5.8.1.4: on newlyMatching "a copy of the original
...                 Subscription shall be reduced to what is matched by the
...                 registration information ... forwarded to the Context
...                 Source as a new Subscription where the notification
...                 endpoint is set to that of the local Broker". 5.8.2.4:
...                 the CSR Subscription "shall be updated" on Subscription
...                 update; isActive=false sets status "paused". 5.8.6:
...                 "Notifications shall only be sent if and only if the
...                 status of the corresponding subscription is active";
...                 timeInterval notifications "shall include all the
...                 subscribed Entities that match"; change-driven
...                 notifications include "all the subscribed Entities that
...                 changed and that match" the q conditions.

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
${notify_port}      8097


*** Test Cases ***
IOP_EXT_SUB_01_01 Subscription Update Re-Narrows The Chain
    [Documentation]    5.8.2.4: "Based on the mapping of the Subscription to
    ...    its respective Context Source Registration Subscription (see
    ...    clause 5.8.1.4), that Context Source Registration Subscription
    ...    shall be updated" — after tightening q, a change matching only
    ...    the OLD q no longer notifies.
    [Tags]    iop    iop-ext    5_8_2    5_8_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "q": "speed>10", "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    50
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200

    ${fragment}=    Evaluate    {"q": "speed>100"}
    ${response}=    Patch Subscription At Broker    ${b1_url}    ${subscription_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)
    Sleep    1s
    ${old_only}=    Evaluate    {"speed": {"type": "Property", "value": 60}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${old_only}
    Sleep    1s
    ${new_q}=    Evaluate    {"speed": {"type": "Property", "value": 150}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${new_q}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    150
    Should Not Contain    ${body.decode('utf-8')}    "value":60

IOP_EXT_SUB_01_02 isActive=false Pauses The Chain, Reactivation Resumes
    [Documentation]    5.8.2.4: isActive=false updates status to "paused";
    ...    5.8.6: "Notifications shall only be sent if and only if the
    ...    status of the corresponding subscription is active, i.e. not
    ...    paused nor expired."
    [Tags]    iop    iop-ext    5_8_2    5_8_6    5_2_12    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    41
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200

    ${off}=    Evaluate    {"isActive": False}
    ${response}=    Patch Subscription At Broker    ${b1_url}    ${subscription_id}    ${off}
    Should Be True    ${response.status_code} in (204, 207)
    Sleep    1s
    ${paused}=    Evaluate    {"speed": {"type": "Property", "value": 42}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${paused}
    Sleep    1s
    ${on}=    Evaluate    {"isActive": True}
    ${response}=    Patch Subscription At Broker    ${b1_url}    ${subscription_id}    ${on}
    Should Be True    ${response.status_code} in (204, 207)
    Sleep    1s
    ${resumed}=    Evaluate    {"speed": {"type": "Property", "value": 43}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${resumed}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    43
    Should Not Contain    ${body.decode('utf-8')}    "value":42

IOP_EXT_SUB_01_03 The Reduced Remote Copy Watches Only Registered Attributes
    [Documentation]    5.8.1.4: "a copy of the original Subscription shall
    ...    be reduced to what is matched by the registration information" —
    ...    the registration offers only speed, so a remote brandName change
    ...    does not notify; a speed change does.
    [Tags]    iop    iop-ext    5_8_1    5_2_10    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"type": $etype}], "propertyNames": ["speed"]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    ${e}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 7}, "brandName": {"type": "Property", "value": "Alpha"}}
    Create Entity At Broker    ${b2_url}    ${e}
    Start Notify Server
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s

    ${unwatched}=    Evaluate    {"brandName": {"type": "Property", "value": "Zeta"}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${unwatched}
    Sleep    1s
    ${watched}=    Evaluate    {"speed": {"type": "Property", "value": 99}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${watched}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    "value":99
    Should Not Contain    ${body.decode('utf-8')}    "value":7

IOP_EXT_SUB_01_04 A timeInterval Subscription Notifies The Federated Union
    [Documentation]    5.8.6: "If a Subscription defines a timeInterval
    ...    member, a Notification shall be sent periodically ... The
    ...    notification message shall include all the subscribed Entities
    ...    that match" — on a distributing broker the subscribed entities
    ...    include those held by registered sources.
    [Tags]    iop    iop-ext    5_8_6    5_2_12    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    Create Entity At Broker    ${b2_url}    ${e}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "timeInterval": 2, "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${entity_id}
    Should Contain    ${body.decode('utf-8')}    Notification
    Should Not Contain    ${body.decode('utf-8')}    ${entity_id}-x

IOP_EXT_SUB_01_05 Remote Values Arrive Unmodified At The Subscriber
    [Documentation]    5.8.6/5.3.1: the notification data member carries the
    ...    NGSI-LD Entities — remote attribute values reach the subscriber
    ...    verbatim, with no broker-injected members inside the entity.
    [Tags]    iop    iop-ext    5_8_6    5_3_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s
    ${e}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 12345}, "brandName": {"type": "Property", "value": "Omega"}}
    Create Entity At Broker    ${b2_url}    ${e}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    ${doc}=    Evaluate    __import__('json').loads($body.decode('utf-8'))
    Length Should Be    ${doc['data']}    1
    ${entity}=    Evaluate    $doc['data'][0]
    Should Be Equal As Integers    ${entity['speed']['value']}    12345
    Should Be Equal    ${entity['brandName']['value']}    Omega
    ${extra}=    Evaluate
    ...    [k for k in $entity if k not in ("id", "type", "speed", "brandName", "@context")]
    Should Be Empty    ${extra}

IOP_EXT_SUB_01_06 Notification Fires When A Remote Entity Enters The Filter
    [Documentation]    5.8.6: the notification includes "all the subscribed
    ...    Entities that changed and that match ... the query" — a change
    ...    that leaves the entity outside q notifies nothing; the change
    ...    that brings it inside does. (Reworded from triggerReason: Table
    ...    5.3.1-1 defines no triggerReason member on entity Notifications
    ...    — it exists only on CSourceNotification, 5.3.2/5.3.3.)
    [Tags]    iop    iop-ext    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    Create Entity At Broker    ${b2_url}    ${e}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "q": "speed>100", "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${outside}=    Evaluate    {"speed": {"type": "Property", "value": 50}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${outside}
    Sleep    1s
    ${inside}=    Evaluate    {"speed": {"type": "Property", "value": 150}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${inside}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    "value":150
    Should Not Contain    ${body.decode('utf-8')}    "value":50

IOP_EXT_SUB_01_07 No Notification When The Remote Update Leaves The Filter
    [Documentation]    5.8.6: only entities "that changed and that match"
    ...    the q conditions are included — an update that takes the entity
    ...    OUT of the filter produces no notification; re-entering does.
    ...    (Reworded from triggerReason=noLongerMatching: entity
    ...    Notifications carry no triggerReason, Table 5.3.1-1.)
    [Tags]    iop    iop-ext    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    150
    Create Entity At Broker    ${b2_url}    ${e}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "q": "speed>100", "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${leaves}=    Evaluate    {"speed": {"type": "Property", "value": 50}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${leaves}
    Sleep    1s
    ${reenters}=    Evaluate    {"speed": {"type": "Property", "value": 200}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${reenters}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    "value":200
    Should Not Contain    ${body.decode('utf-8')}    "value":50

IOP_EXT_SUB_01_08 A Remote Entity Deletion Notifies With deletedAt Only
    [Documentation]    5.2.12: notificationTrigger default is
    ...    attributeCreated+attributeUpdated only — deletions notify when
    ...    "entityDeleted" is listed. 5.8.6: "If the notification was
    ...    triggered by the deletion of an Entity and the
    ...    notification.showChanges member is not set to true, only the
    ...    deletedAt system property shall be provided."
    [Tags]    iop    iop-ext    5_8_6    5_2_12    4_5_7    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "notificationTrigger": ["attributeCreated", "attributeUpdated", "entityDeleted"], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    31
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200

    ${response}=    Delete Entity Via Broker    ${b2_url}    ${entity_id}
    Should Be True    ${response.status_code} in (204, 207)

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${entity_id}
    Should Contain    ${body.decode('utf-8')}    deletedAt
    Should Not Contain    ${body.decode('utf-8')}    "value":31


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopSub${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopSub:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopsub-${suffix}
    Set Test Variable    ${subscription_id}    urn:ngsi-ld:Subscription:iopsub-${suffix}
    Set Test Variable    ${server_started}    ${False}

Start Notify Server
    Start Server    ${notify_host}    ${notify_port}
    Set Test Variable    ${server_started}    ${True}

Post Subscription At Broker
    [Arguments]    ${at}    ${sub}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${at}/subscriptions    json=${sub}    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Patch Subscription At Broker
    [Arguments]    ${at}    ${sub_id}    ${fragment}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${sid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($sub_id, safe='')
    ${response}=    PATCH    url=${at}/subscriptions/${sid}    json=${fragment}
    ...    headers=${headers}    expected_status=any
    RETURN    ${response}

Cleanup Interop Fixtures
    Delete Subscription At Broker    ${b1_url}    ${subscription_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    FOR    ${at}    IN    ${b1_url}    ${b2_url}    ${b3_url}
        Delete Entity Via Broker    ${at}    ${entity_id}
    END
    IF    ${server_started}
        Stop Server
    END
