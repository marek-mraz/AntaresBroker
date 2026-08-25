*** Settings ***
Documentation       Advanced multi-broker scenarios II (Antares extension
...                 IOP TPs): DISTRIBUTED SUBSCRIPTIONS (entity changes in
...                 B2 notify a subscriber of B1 — 5.8.1.4), tenant-scoped
...                 federated queries, warning behaviour on dead peers,
...                 exclusive registrations mixed with local attributes,
...                 expiring registrations, partial batch success (207) and
...                 GeoJSON over the federated union.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             HttpCtrl.Server

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${notify_host}      127.0.0.1
${notify_port}      8093


*** Test Cases ***
IOP_EXT_ADV_02_01 Entity Created In Broker2 Notifies Broker1's Subscriber
    [Documentation]    5.8.1.4: a Subscription at B1 + a matching
    ...    registration to B2 — creating the entity DIRECTLY in B2 flows
    ...    B2 → B1 → subscriber: the notification carries the entity.
    [Tags]    iop    iop-ext    5_8_1    4_3_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Server    ${notify_host}    ${notify_port}
    Set Test Variable    ${server_started}    ${True}
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s

    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${entity_id}
    Should Contain    ${body.decode('utf-8')}    Notification

IOP_EXT_ADV_02_02 Attribute Update In Broker2 Notifies With The New Value
    [Documentation]    5.8.1.4: the entity already exists in B2; updating
    ...    its Attribute directly in B2 produces a notification at B1's
    ...    subscriber carrying the NEW value.
    [Tags]    iop    iop-ext    5_8_1    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}
    Start Server    ${notify_host}    ${notify_port}
    Set Test Variable    ${server_started}    ${True}
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s

    ${fragment}=    Evaluate    {"speed": {"type": "Property", "value": 88}}
    ${response}=    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${entity_id}
    Should Contain    ${body.decode('utf-8')}    88

IOP_EXT_ADV_02_03 Tenant-Scoped Federated Query
    [Documentation]    4.14/6.3.14 (query flavour of the retrieve case):
    ...    B2's iop-tenant entities reach B1's default tenant through the
    ...    registration tenant member.
    [Tags]    iop    iop-ext    4_14    6_3_14    since_v1.9.1
    ${tenant}=    Set Variable    iop${suffix}
    Set Test Variable    ${used_tenant}    ${tenant}
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    reg_tenant=${tenant}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}    tenant=${tenant}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}
    ${response}=    Query Entities Via Broker    ${b2_url}    type=${etype}
    Should Not Contain    ${response.text}    ${entity_id}

IOP_EXT_ADV_02_04 Dead Peer On Retrieve Warns But Serves Local Data
    [Documentation]    6.3.17 (retrieve flavour): the local half is served
    ...    200 with an NGSILD-Warning for the unreachable registration part.
    [Tags]    iop    iop-ext    5_7_1    6_3_17    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    http://127.0.0.1:59999/ngsi-ld/v1    ${etype}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b1_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    42
    Dictionary Should Contain Key    ${response.headers}    NGSILD-Warning

IOP_EXT_ADV_02_05 Exclusive Remote Attributes Merge With Local Ones
    [Documentation]    4.3.6.3: speed is EXCLUSIVELY owned by B2 (the
    ...    registration names id + attribute); B1 legitimately holds the
    ...    unregistered brandName locally — retrieval via B1 merges both.
    [Tags]    iop    iop-ext    4_3_6_3    4_5_5    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=exclusive    information=${info}
    ${remote}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${remote}
    ${local}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b1_url}    ${local}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    speed
    Dictionary Should Contain Key    ${response.json()}    brandName

IOP_EXT_ADV_02_06 Redirect Data Appears Exactly Once In A Query
    [Documentation]    4.3.6.3: with a redirect registration and no local
    ...    copy, the federated query serves B2's entity exactly once —
    ...    no duplication between the local and remote arms.
    [Tags]    iop    iop-ext    4_3_6_3    5_7_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    ${occurrences}=    Evaluate    $response.text.count('"' + $entity_id + '"')
    Should Be Equal As Integers    ${occurrences}    1

IOP_EXT_ADV_02_07 An Expired Registration Stops Matching
    [Documentation]    5.2.9 expiresAt: the registration expires after two
    ...    seconds — B2's entity is reachable before, gone after; the
    ...    federated view honours registration lifetimes.
    [Tags]    iop    iop-ext    5_2_9    5_7_2    since_v1.9.1
    ${expires}=    Evaluate
    ...    (__import__('datetime').datetime.utcnow() + __import__('datetime').timedelta(seconds=2)).strftime('%Y-%m-%dT%H:%M:%SZ')
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "inclusive", "information": $info, "endpoint": __import__('re').sub("/ngsi-ld/v1$", "", $b2_url), "expiresAt": $expires}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${b1_url}/csourceRegistrations    json=${reg}    headers=${headers}    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Should Contain    ${response.text}    ${entity_id}

    Sleep    3s
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${entity_id}

IOP_EXT_ADV_02_08 Partial Batch Success Is A 207 With The Local Item Stored
    [Documentation]    5.6.7/6.3.17: one batch item matches a READ-ONLY
    ...    redirect registration (Conflict part), the other is purely local
    ...    — the batch answers 207, the local item exists, the redirected
    ...    one is nowhere.
    [Tags]    iop    iop-ext    5_6_7    6_3_17    since_v1.9.1
    ${read_ops}=    Evaluate    ["retrieveEntity", "queryEntity"]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect    operations=${read_ops}

    ${batch}=    Evaluate
    ...    [{"id": $entity_id + "-remote", "type": $etype, "speed": {"type": "Property", "value": 1}}, {"id": $entity_id + "-local", "type": $etype + "Other", "speed": {"type": "Property", "value": 2}}]
    ${response}=    Batch Op Via Broker    ${b1_url}    create    ${batch}
    Check Response Status Code    207    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}-local    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}-remote    local=true
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}-remote    local=true
    Check Response Status Code    404    ${response.status_code}

IOP_EXT_ADV_02_09 GeoJSON FeatureCollection Over The Federated Union
    [Documentation]    4.5.17/6.3.15: Accept application/geo+json on a
    ...    federated query — the FeatureCollection carries Features from
    ...    both brokers.
    [Tags]    iop    iop-ext    4_5_17    6_3_15    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${l}=    Evaluate
    ...    {"id": $entity_id + "-l", "type": $etype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 1.0]}}}
    Create Entity At Broker    ${b1_url}    ${l}
    ${r}=    Evaluate
    ...    {"id": $entity_id + "-r", "type": $etype, "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.0, 2.0]}}}
    Create Entity At Broker    ${b2_url}    ${r}

    &{headers}=    Create Dictionary    Accept=application/geo+json
    ${response}=    GET
    ...    url=${b1_url}/entities
    ...    params=type=${etype}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['type']}    FeatureCollection
    Length Should Be    ${response.json()['features']}    2

IOP_EXT_ADV_02_10 Deleted Subscription Stops The Distributed Chain
    [Documentation]    5.8.5 through 5.8.1.4: after the subscription at B1
    ...    is deleted, a further change in B2 must NOT notify the old
    ...    subscriber endpoint; a fresh subscription proves the chain itself
    ...    still works (guarding against a silently-dead fixture).
    [Tags]    iop    iop-ext    5_8_5    5_8_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Server    ${notify_host}    ${notify_port}
    Set Test Variable    ${server_started}    ${True}
    Create Subscription At Broker    ${b1_url}    ${subscription_id}    ${etype}
    ...    http://${notify_host}:${notify_port}/notify
    Sleep    1s
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${entity}
    Wait For Request    ${30}
    Reply By    200

    Delete Subscription At Broker    ${b1_url}    ${subscription_id}
    Sleep    1s
    ${fragment}=    Evaluate    {"speed": {"type": "Property", "value": 2}}
    ${response}=    Patch Entity Attrs Via Broker    ${b2_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    ${status}=    Run Keyword And Return Status    Wait For Request    ${5}
    IF    ${status}
        Reply By    200
    END
    Should Not Be True    ${status}    msg=no notification may arrive after the subscription is deleted


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}
    Set Test Variable    ${subscription_id}    urn:ngsi-ld:Subscription:iopext-${suffix}
    Set Test Variable    ${used_tenant}    ${EMPTY}
    Set Test Variable    ${server_started}    ${False}

Cleanup Interop Fixtures
    Delete Subscription At Broker    ${b1_url}    ${subscription_id}
    FOR    ${tail}    IN    ${EMPTY}    -l    -r    -remote    -local
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
    END
    IF    '${used_tenant}' != ''
        Delete Entity Via Broker    ${b2_url}    ${entity_id}    tenant=${used_tenant}
    END
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    IF    ${server_started}
        Stop Server
    END
