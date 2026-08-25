*** Settings ***
Documentation       Distributed subscriptions II (Antares extension IOP
...                 TPs). 5.8.6 splitEntities inbound merge: "The Entities
...                 contained in the data member of the Notification shall
...                 be retrieved locally and from all Context Sources that
...                 have information about these Entities, except for the
...                 one from which the Notification has been received. The
...                 retrieved Entities then shall be merged ... All
...                 Entities that do not match the query, geoquery and
...                 Scope query conditions of the local Subscription shall
...                 be removed"; forwarding uses "this local Subscription
...                 identifier instead of the subscriptionId received" and
...                 only happens for notifications "with a subscriptionId
...                 ... that has a mapping to a local Subscription
...                 identifier". 5.8.1.4: registration-triggered wiring via
...                 the CSR Subscription (newlyMatching). 5.8.5.4: delete
...                 forwarded per mapped Context Source.

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
IOP_EXT_SUB_02_01 splitEntities Merges The Notification From All Sources
    [Documentation]    5.8.6: with splitEntities the notified Entities
    ...    "shall be retrieved locally and from all Context Sources that
    ...    have information about these Entities, except for the one from
    ...    which the Notification has been received" and merged — the
    ...    subscriber receives ONE entity assembled from B2's and B3's
    ...    halves.
    [Tags]    iop    iop-ext    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}
    ${half2}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${half2}
    ${half3}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b3_url}    ${half3}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "splitEntities": True, "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${change}=    Evaluate    {"speed": {"type": "Property", "value": 43}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${change}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    ${doc}=    Evaluate    __import__('json').loads($body.decode('utf-8'))
    Length Should Be    ${doc['data']}    1
    ${entity}=    Evaluate    $doc['data'][0]
    Should Be Equal As Integers    ${entity['speed']['value']}    43
    Should Be Equal    ${entity['brandName']['value']}    Mercedes
    Should Be Equal    ${doc['subscriptionId']}    ${subscription_id}

IOP_EXT_SUB_02_02 splitEntities Merge Applies q To The Aggregate
    [Documentation]    5.8.6: after the merge, "All Entities that do not
    ...    match the query ... conditions of the local Subscription shall
    ...    be removed from the data member. If there are Entities in the
    ...    data member ... the Notification copy shall be forwarded" — a
    ...    change failing the aggregate q notifies nothing; the passing
    ...    change notifies the merged entity.
    [Tags]    iop    iop-ext    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}
    ${half2}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${half2}
    ${half3}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b3_url}    ${half3}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "splitEntities": True, "q": "speed>20;brandName==\\"Mercedes\\"", "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${fails}=    Evaluate    {"speed": {"type": "Property", "value": 10}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${fails}
    Sleep    1s
    ${passes}=    Evaluate    {"speed": {"type": "Property", "value": 42}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${passes}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    "value":42
    Should Contain    ${body.decode('utf-8')}    Mercedes
    Should Not Contain    ${body.decode('utf-8')}    "value":10

IOP_EXT_SUB_02_03 A Registration Created After The Subscription Wires The Chain
    [Documentation]    5.8.1.4: the CSR Subscription notifies newlyMatching
    ...    "whenever a new Context Source Registration matching the
    ...    Subscription has been registered" (5.3.3) — the reduced copy is
    ...    forwarded then, so a subscription created BEFORE the CSR still
    ...    ends up notified of remote changes.
    [Tags]    iop    iop-ext    5_8_1    5_3_3    since_v1.9.1
    Start Notify Server
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Sleep    1s

    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    64
    Create Entity At Broker    ${b2_url}    ${e}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${entity_id}
    Should Contain    ${body.decode('utf-8')}    "value":64
    Should Not Contain    ${body.decode('utf-8')}    ${entity_id}-x

IOP_EXT_SUB_02_04 Deleting The Registration Dismantles The Remote Leg
    [Documentation]    5.8.1.4/5.3.3 noLongerMatching ("because it was
    ...    deleted"): "a delete Subscription shall be forwarded to the
    ...    Context Source" — after the CSR is deleted, remote changes stop
    ...    notifying while local changes still do.
    [Tags]    iop    iop-ext    5_8_1    5_11_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    51
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200

    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Sleep    1s
    ${remote}=    Evaluate    {"speed": {"type": "Property", "value": 666}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${remote}
    Sleep    1s
    ${local}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    52
    Create Entity At Broker    ${b1_url}    ${local}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${entity_id}-l
    Should Not Contain    ${body.decode('utf-8')}    "value":666

IOP_EXT_SUB_02_05 Two Subscriptions Share One Source; Deleting One Keeps The Other
    [Documentation]    5.8.5.4: the delete is forwarded "using the mapping
    ...    of the own Subscription identifier" — per subscription. The
    ...    second subscription's chain over the same registration survives.
    [Tags]    iop    iop-ext    5_8_5    5_8_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/n1
    Create Subscription At Broker    ${b1_url}    ${subscription_id}-2    ${etype}
    ...    http://${notify_host}:${notify_port}/n2
    Sleep    1s
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    61
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200
    Wait For Request    ${30}
    Reply By    200

    Delete Subscription At Broker    ${b1_url}    ${subscription_id}
    Sleep    1s
    ${change}=    Evaluate    {"speed": {"type": "Property", "value": 777}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${change}

    Wait For Request    ${30}
    ${url}=    Get Request Url
    ${body}=    Get Request Body
    Reply By    200
    Should Be Equal    ${url}    /n2
    Should Contain    ${body.decode('utf-8')}    "value":777
    Should Contain    ${body.decode('utf-8')}    ${subscription_id}-2

IOP_EXT_SUB_02_06 An Unknown Inbound subscriptionId Is Never Delivered
    [Documentation]    5.8.6: forwarding happens only "If a Notification
    ...    with a subscriptionId is received that has a mapping to a local
    ...    Subscription identifier" — a forged/stale subscriptionId is
    ...    dropped, and the real chain keeps working.
    [Tags]    iop    iop-ext    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    71
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200

    ${subs}=    GET    url=${b2_url}/subscriptions    expected_status=any
    ${inbound}=    Evaluate
    ...    next(s['notification']['endpoint']['uri'] for s in $subs.json() if 'distsub' in s['id'])
    ${forged}=    Evaluate
    ...    {"id": "urn:ngsi-ld:Notification:forged", "type": "Notification", "subscriptionId": "urn:ngsi-ld:Subscription:unknown-" + $suffix, "notifiedAt": "2026-08-14T00:00:00Z", "data": [{"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 31337}}]}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${inbound}    json=${forged}    headers=${headers}    expected_status=any
    Should Be True    ${response.status_code} < 500
    Wait For No Request    ${3}

    ${change}=    Evaluate    {"speed": {"type": "Property", "value": 888}}
    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${change}
    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    "value":888
    Should Not Contain    ${body.decode('utf-8')}    31337


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopSuc${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopSuc:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopsuc-${suffix}
    Set Test Variable    ${subscription_id}    urn:ngsi-ld:Subscription:iopsuc-${suffix}
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

Cleanup Interop Fixtures
    Delete Subscription At Broker    ${b1_url}    ${subscription_id}
    Delete Subscription At Broker    ${b1_url}    ${subscription_id}-2
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-3
    FOR    ${tail}    IN    ${EMPTY}    -l
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b3_url}    ${entity_id}${tail}
    END
    IF    ${server_started}
        Stop Server
    END
