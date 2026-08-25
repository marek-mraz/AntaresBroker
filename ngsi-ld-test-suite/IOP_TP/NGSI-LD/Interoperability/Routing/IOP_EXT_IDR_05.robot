*** Settings ***
Documentation       Subscriptions and notifications routed by EntityInfo
...                 id/idPattern (Antares extension IOP TPs, ADR-001 URN
...                 vocabulary). 5.8.1.4: on newlyMatching "a copy of the
...                 original Subscription shall be reduced to what is
...                 matched by the registration information … forwarded to
...                 the Context Source as a new Subscription where the
...                 notification endpoint is set to that of the local
...                 Broker". 5.12 governs the Subscription-vs-CSR matching
...                 (including "Both a specified id pattern and an
...                 idPattern in the Entity Info are present" ⇒ assumed
...                 compatible — NOTE: two disjoint-looking patterns still
...                 match; only a sub pattern vs an exact EntityInfo id is
...                 decidably non-matching). 5.2.33 EntitySelector: "id
...                 takes precedence over idPattern". 5.2.12
...                 notificationTrigger: default is entityCreated +
...                 attribute triggers — "entityDeleted" fires only when
...                 requested. 5.11.3 csourceSubscriptions reuse the same
...                 5.12 matching for csource notifications.

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
${notify_host}      127.0.0.1
${notify_port}      8097


*** Test Cases ***
IOP_EXT_IDR_05_40 An Overlapping Pattern Subscription Spawns The Remote Chain
    [Documentation]    5.8.1.4: the subscription's idPattern overlaps the
    ...    CSR's pattern (5.12: both patterns present ⇒ compatible) — a
    ...    reduced copy is created at B2 (visible in B2's subscription
    ...    list) and a matching create at B2 notifies through B1's chain.
    [Tags]    iop    iop-ext    5_12    5_8_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "idPattern": $pat_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype, "idPattern": $pat_coarse}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${response}=    GET    url=${b2_url}/subscriptions    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${etype}

    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${eid_bb}

IOP_EXT_IDR_05_41 A Sub Pattern Vs An Exact CSR Id Of Another Razidlo Spawns Nothing
    [Documentation]    5.12: the subscription's idPattern anchors sk_presov
    ...    and the CSR's EntityInfo names ONE exact sk_banskabystrica id —
    ...    no match condition holds, so NO remote subscription is created
    ...    at B2 (absence asserted in B2's list) and a create at B2 never
    ...    notifies. (Corrects the naive "disjoint patterns don't match"
    ...    reading: pattern-vs-pattern is always compatible per 5.12;
    ...    pattern-vs-exact-id is the decidable exclusion.)
    [Tags]    iop    iop-ext    5_12    5_8_1    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    information=${info}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype, "idPattern": $pat_presov}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${response}=    GET    url=${b2_url}/subscriptions    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${etype}

    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For No Request    ${3}

IOP_EXT_IDR_05_42 Only The Pattern-Matching Remote Create Notifies
    [Documentation]    5.8.6 over 5.8.1.4: two creates at B2 — one id
    ...    matches the subscription's pattern, one does not. EXACTLY one
    ...    notification arrives and the non-matching id appears in NO
    ...    payload.
    [Tags]    iop    iop-ext    5_12    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype, "idPattern": $pat_bb}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${e}=    Waste Entity    ${eid_presov}    ${0.7}
    Create Entity At Broker    ${b2_url}    ${e}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${eid_bb}
    Should Not Contain    ${body.decode('utf-8')}    ${eid_presov}
    Wait For No Request    ${3}

IOP_EXT_IDR_05_43 A CSource Subscription Filters Registrations By IdPattern
    [Documentation]    5.11.3 + 5.12: a csourceSubscription watching the
    ...    banskabystrica pattern is notified when a CSR whose EntityInfo
    ...    id matches is registered (5.12 pattern-vs-id), and NOT when a
    ...    CSR restricted to a foreign exact id is registered — the
    ...    ADR-001 discovery-automation posture, both halves.
    [Tags]    iop    iop-ext    5_12    5_11_3    since_v1.9.1
    Start Notify Server
    ${csub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype, "idPattern": $pat_bb}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/csnotify", "accept": "application/json"}}}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${b1_url}/csourceSubscriptions    json=${csub}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    Sleep    1s

    ${endpoint}=    Broker Base Of    ${b2_url}
    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_bb}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}
    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${registration_id}

    ${info}=    Evaluate    [{"entities": [{"type": $etype, "id": $eid_presov_exact}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id + "-2", "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint}
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}
    Wait For No Request    ${3}

IOP_EXT_IDR_05_44 A Remote Deletion Notifies Only When entityDeleted Is Requested
    [Documentation]    5.2.12: the default notificationTrigger set does not
    ...    include "entityDeleted" — deleting the remote entity produces NO
    ...    notification; a subscription that requests entityDeleted is
    ...    notified of the deletion.
    [Tags]    iop    iop-ext    5_2_12    5_8_1    5_8_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype, "idPattern": $pat_bb}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200
    Delete Entity Via Broker    ${b2_url}    ${eid_bb}
    Wait For No Request    ${3}
    Delete Subscription At Broker    ${b1_url}    ${subscription_id}
    Sleep    1s

    ${sub}=    Evaluate
    ...    {"id": $subscription_id + "-2", "type": "Subscription", "entities": [{"type": $etype, "idPattern": $pat_bb}], "notificationTrigger": ["entityCreated", "entityDeleted"], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}
    Wait For Request    ${30}
    Reply By    200
    Delete Entity Via Broker    ${b2_url}    ${eid_bb}
    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${eid_bb}

IOP_EXT_IDR_05_45 An Exact Id Selector Beats The Pattern Across The Federation
    [Documentation]    5.2.33: in an EntitySelector carrying both, "id
    ...    takes precedence over idPattern" — the presov create matching
    ...    only the pattern must NOT notify; the listed exact id does.
    ...    Exactly one notification, the pattern-only id in no payload.
    [Tags]    iop    iop-ext    5_2_33    5_8_1    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Start Notify Server
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype, "id": $eid_bb, "idPattern": $pat_presov}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/notify", "accept": "application/json"}}}
    Post Subscription At Broker    ${b1_url}    ${sub}
    Sleep    1s

    ${e}=    Waste Entity    ${eid_presov}    ${0.7}
    Create Entity At Broker    ${b2_url}    ${e}
    ${e}=    Waste Entity    ${eid_bb}    ${0.42}
    Create Entity At Broker    ${b2_url}    ${e}

    Wait For Request    ${30}
    ${body}=    Get Request Body
    Reply By    200
    Should Contain    ${body.decode('utf-8')}    ${eid_bb}
    Should Not Contain    ${body.decode('utf-8')}    ${eid_presov}
    Wait For No Request    ${3}


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    WasteC${suffix}
    Set Test Variable    ${base}    urn:ngsi-ld:WasteC${suffix}
    ${pat_coarse}=    Evaluate    "^" + $base + ":sk_banskabystrica:.*$"
    Set Test Variable    ${pat_coarse}
    ${pat_bb}=    Evaluate    "^" + $base + ":sk_banskabystrica:odpady:.*$"
    Set Test Variable    ${pat_bb}
    ${pat_presov}=    Evaluate    "^" + $base + ":sk_presov:odpady:.*$"
    Set Test Variable    ${pat_presov}
    Set Test Variable    ${eid_bb}    ${base}:sk_banskabystrica:odpady:kontajner:0042
    Set Test Variable    ${eid_presov}    ${base}:sk_presov:odpady:kontajner:0001
    Set Test Variable    ${eid_presov_exact}    ${base}:sk_presov:odpady:kontajner:0002
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopidr-${suffix}
    Set Test Variable    ${subscription_id}    urn:ngsi-ld:Subscription:iopidr-${suffix}
    Set Test Variable    ${server_started}    ${False}

Waste Entity
    [Documentation]    ADR-001-shaped WasteContainer fixture.
    [Arguments]    ${eid}    ${level}
    ${e}=    Evaluate
    ...    {"id": $eid, "type": $etype, "fillLevel": {"type": "Property", "value": $level}}
    RETURN    ${e}

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
    ${sid}=    Evaluate    __import__('urllib.parse', fromlist=['quote']).quote($subscription_id, safe='')
    ${response}=    DELETE    url=${b1_url}/csourceSubscriptions/${sid}    expected_status=any
    FOR    ${rid}    IN    ${registration_id}    ${registration_id}-2
        Delete Registration At Broker    ${b1_url}    ${rid}
    END
    FOR    ${eid}    IN    ${eid_bb}    ${eid_presov}    ${eid_presov_exact}
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
    END
    IF    ${server_started}
        Stop Server
    END
