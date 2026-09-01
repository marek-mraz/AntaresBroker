*** Settings ***
Documentation       Verify 5.8.1.4 / 5.8.5.4: the consumer half of
...                 distributed subscriptions. Creating an entity
...                 Subscription (localOnly != true) creates a Context
...                 Source Registration Subscription (5.11.2) and forwards a
...                 reduced copy of the Subscription to a matching
...                 registration supporting createSubscription — with the
...                 notification endpoint set to the local broker; deleting
...                 the Subscription forwards the delete (5.11.6).
...                 Antares extension TP — no official coverage of the
...                 consumer half.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Registration And Start Mock
Test Teardown       Clean Up


*** Variables ***
${sub_id}=      urn:ngsi-ld:Subscription:distsub5814


*** Test Cases ***
5814_01_01 Subscription Forwarded To Matching Context Source And Deleted
    [Documentation]    5.8.1.4: a reduced copy of the Subscription reaches
    ...    the registered Context Source with the local broker as its
    ...    notification endpoint (never the original subscriber's); the
    ...    delete is forwarded (5.8.5.4). The Context Source Registration
    ...    Subscription 5.8.1.4 creates is the broker's own, addressed to
    ...    the broker itself, so nothing here asserts it is served on the
    ...    5.11 endpoints.
    [Tags]    dist-ops    5_8_1_4    5_8_5_4    since_v1.9.1

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${sub}=    Set Variable
    ...    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "q": "speed>50", "notification": {"endpoint": {"uri": "http://original.subscriber.example:9998/notify"}}}
    ${response}=    POST    url=${url}/subscriptions    data=${sub}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    # the reduced copy arrives at the Context Source
    Wait For Request    ${15}
    ${method}=    Get Request Method
    ${path}=    Get Request Url
    ${body}=    Get Request Body
    Should Be Equal    ${method}    POST
    Should Contain    ${path}    /ngsi-ld/v1/subscriptions
    ${body}=    Evaluate    $body.decode('utf-8') if isinstance($body, bytes) else $body
    # 5.8.1.4: "the notification endpoint is set to that of the local
    # Broker" — the copy notifies the broker, which remaps and forwards to
    # the subscriber. The URL that endpoint carries is not standardized, so
    # what is asserted is that it exists and is NOT the subscriber's own.
    ${copy_uri}=    Evaluate    __import__('json').loads($body)["notification"]["endpoint"]["uri"]
    Should Not Be Empty    ${copy_uri}
    Should Not Contain    ${copy_uri}    original.subscriber.example
    Reply By    201

    # 5.8.5.4: the delete is forwarded with the mapped subscriptionId
    ${response}=    DELETE    url=${url}/subscriptions/${sub_id}    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    Wait For Request    ${15}
    ${method}=    Get Request Method
    ${path}=    Get Request Url
    Should Be Equal    ${method}    DELETE
    Should Contain    ${path}    /ngsi-ld/v1/subscriptions/
    Reply By    204

5814_01_02 Split Entities Inbound Notification Is Merged And Refiltered
    [Documentation]    5.8.6 splitEntities=true: the reduced remote copy
    ...    carries no q; the Entities of an inbound Notification are
    ...    retrieved locally, merged with the notified fragments, re-filtered
    ...    by the local Subscription's q, and forwarded under the local
    ...    subscriptionId; fragments failing q after the merge are dropped.
    [Tags]    dist-ops    5_8_6    since_v1.9.1

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${sub}=    Set Variable
    ...    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "q": "speed>50;brandName==%22Tesla%22", "splitEntities": true, "notification": {"endpoint": {"uri": "http://${context_source_host}:${context_source_port}/notify"}}}
    ${response}=    POST    url=${url}/subscriptions    data=${sub}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    # the reduced copy must NOT push the q down (5.8.1.4 with splitEntities)
    Wait For Request    ${15}
    ${body}=    Get Request Body
    ${body}=    Evaluate    $body.decode('utf-8') if isinstance($body, bytes) else $body
    Should Not Contain    ${body}    "q"
    Should Contain    ${body}    "splitEntities":true
    ${remote_id}=    Evaluate    __import__('json').loads($body)["id"]
    ${notify_url}=    Evaluate    __import__('json').loads($body)["notification"]["endpoint"]["uri"]
    Reply By    201

    # the LOCAL part of the entity carries the brandName the q needs
    ${response}=    POST    url=${url}/entities
    ...    data={"id": "urn:ngsi-ld:Vehicle:split5814", "type": "Vehicle", "brandName": {"type": "Property", "value": "Tesla"}}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    # inbound fragment (speed only) → merged with the local brandName → q passes
    ${response}=    POST    url=${notify_url}
    ...    data={"type": "Notification", "subscriptionId": "${remote_id}", "data": [{"id": "urn:ngsi-ld:Vehicle:split5814", "type": "Vehicle", "speed": {"type": "Property", "value": 99}}]}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Wait For Request    ${15}
    ${npath}=    Get Request Url
    ${nbody}=    Get Request Body
    ${nbody}=    Evaluate    $nbody.decode('utf-8') if isinstance($nbody, bytes) else $nbody
    Should Contain    ${npath}    /notify
    Should Contain    ${nbody}    urn:ngsi-ld:Vehicle:split5814
    Should Contain    ${nbody}    Tesla
    Should Contain    ${nbody}    ${sub_id}
    Should Not Contain    ${nbody}    ${remote_id}
    Reply By    200

    # a fragment failing q after the merge (speed 10) is dropped entirely
    ${response}=    POST    url=${notify_url}
    ...    data={"type": "Notification", "subscriptionId": "${remote_id}", "data": [{"id": "urn:ngsi-ld:Vehicle:split5814", "type": "Vehicle", "speed": {"type": "Property", "value": 10}}]}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Run Keyword And Expect Error    *    Wait For Request    ${3}

    ${response}=    DELETE    url=${url}/entities/urn:ngsi-ld:Vehicle:split5814    expected_status=any
    Check Response Status Code    204    ${response.status_code}


5814_01_03 Split Entities Inbound Notification Carries A Notified Deletion
    [Documentation]    5.5.4 bans "urn:ngsi-ld:null" as a first level member
    ...    value "with the exception of NGSI-LD Fragments ... or to represent
    ...    deleted Properties in concise representation as part of
    ...    notifications". A Context Source therefore reports a deletion
    ...    (4.5.7) as a concise NGSI-LD Null, and the 5.8.6 splitEntities
    ...    merge shall carry that Entity through to the original subscriber
    ...    rather than refuse it as an invalid payload. Deleted Relationships
    ...    and Language Properties travel in their own encodings.
    [Tags]    dist-ops    5_8_6    5_5_4    4_5_7    since_v1.9.1

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${sub}=    Set Variable
    ...    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "splitEntities": true, "notification": {"endpoint": {"uri": "http://${context_source_host}:${context_source_port}/notify"}}}
    ${response}=    POST    url=${url}/subscriptions    data=${sub}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    Wait For Request    ${15}
    ${body}=    Get Request Body
    ${body}=    Evaluate    $body.decode('utf-8') if isinstance($body, bytes) else $body
    ${remote_id}=    Evaluate    __import__('json').loads($body)["id"]
    ${notify_url}=    Evaluate    __import__('json').loads($body)["notification"]["endpoint"]["uri"]
    Reply By    201

    # the Context Source reports speed, isParked and label deleted, and a
    # live brandName on the same Entity
    ${notified}=    Set Variable
    ...    {"type": "Notification", "subscriptionId": "${remote_id}", "data": [{"id": "urn:ngsi-ld:Vehicle:splitnull5814", "type": "Vehicle", "speed": "urn:ngsi-ld:null", "isParked": {"object": "urn:ngsi-ld:null"}, "label": {"languageMap": {"@none": "urn:ngsi-ld:null"}}, "brandName": "Tesla"}]}
    ${response}=    POST    url=${notify_url}
    ...    data=${notified}    headers=${headers}    expected_status=any
    Check Response Status Code    200    ${response.status_code}

    Wait For Request    ${15}
    ${nbody}=    Get Request Body
    ${nbody}=    Evaluate    $nbody.decode('utf-8') if isinstance($nbody, bytes) else $nbody
    ${ent}=    Evaluate    __import__('json').loads($nbody)["data"][0]
    Should Be Equal    ${ent}[speed][value]    urn:ngsi-ld:null
    Should Be Equal    ${ent}[isParked][object]    urn:ngsi-ld:null
    Should Be Equal    ${ent}[label][languageMap][@none]    urn:ngsi-ld:null
    Should Be Equal    ${ent}[brandName][value]    Tesla
    Reply By    200


*** Keywords ***
Setup Registration And Start Mock
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${reg}=    Set Variable
    ...    {"id": "${registration_id}", "type": "ContextSourceRegistration", "information": [{"entities": [{"type": "Vehicle"}]}], "operations": ["federationOps"], "endpoint": "http://${context_source_host}:${context_source_port}"}
    ${response}=    POST    url=${url}/csourceRegistrations    data=${reg}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Clean Up
    ${response}=    DELETE    url=${url}/subscriptions/${sub_id}    expected_status=any
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
