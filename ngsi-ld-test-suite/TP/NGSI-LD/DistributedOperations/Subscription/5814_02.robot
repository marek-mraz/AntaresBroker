*** Settings ***
Documentation       Verify 6.3.17 / 6.3.18 on the consumer half of
...                 distributed subscriptions (5.8.1.4): a forwarded
...                 Subscription copy travels with the Via chain of the
...                 brokers it has passed through, extended by each hop; a
...                 copy whose chain already names the receiving broker has
...                 looped back and is NOT re-forwarded, so two mutually
...                 registered brokers cannot create subscription copies
...                 without bound. Antares extension TP — no official
...                 coverage of Via handling on subscription forwarding.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Registration And Start Mock
Test Teardown       Clean Up


*** Variables ***
${sub_id}=      urn:ngsi-ld:Subscription:distsub5814via


*** Test Cases ***
5814_02_01 Forwarded Subscription Copy Extends The Via Chain
    [Documentation]    6.3.17: the broker adds itself to the Via header when
    ...    forwarding. The reduced Subscription copy (5.8.1.4) must arrive at
    ...    the Context Source carrying the inbound chain PLUS this broker's
    ...    own alias, so a downstream broker can detect a loop.
    [Tags]    dist-ops    6_3_17    6_3_18    5_8_1_4    since_v1.9.1

    ${alias}=    Get Broker Alias
    &{headers}=    Create Dictionary    Content-Type=application/json    Via=1.1 upstream-test-hop
    ${sub}=    Set Variable
    ...    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": {"endpoint": {"uri": "http://original.subscriber.example:9998/notify"}}}
    ${response}=    POST    url=${url}/subscriptions    data=${sub}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    Wait For Request    ${15}
    ${method}=    Get Request Method
    ${path}=    Get Request Url
    Should Be Equal    ${method}    POST
    Should Contain    ${path}    /ngsi-ld/v1/subscriptions
    ${req_headers}=    Get Request Headers
    ${via}=    Evaluate    next((v for k, v in dict($req_headers).items() if k.lower() == 'via'), '')
    Should Contain    ${via}    upstream-test-hop
    Should Contain    ${via}    ${alias}
    ${up_pos}=    Evaluate    $via.find('upstream-test-hop')
    ${own_pos}=    Evaluate    $via.find($alias)
    Should Be True    ${up_pos} < ${own_pos}    the inbound hop must precede this broker in the chain: ${via}
    Reply By    201

5814_02_02 A Looped Subscription Copy Is Not Reforwarded
    [Documentation]    6.3.18: the Via header exists "to avoid infinite
    ...    loops". A Subscription created with a Via chain naming THIS broker
    ...    is a copy come full circle: it is created (201) and serves
    ...    locally, but no internal Context Source Registration Subscription
    ...    is created and no copy reaches the registered Context Source. A
    ...    control Subscription without a chain still forwards, so a silent
    ...    harness cannot pass this case.
    [Tags]    dist-ops    6_3_18    5_8_1_4    since_v1.9.1

    ${alias}=    Get Broker Alias
    &{headers}=    Create Dictionary    Content-Type=application/json    Via=1.1 ${alias}
    ${sub}=    Set Variable
    ...    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": {"endpoint": {"uri": "http://original.subscriber.example:9998/notify"}}}
    ${response}=    POST    url=${url}/subscriptions    data=${sub}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

    # no internal CSR subscription, and nothing reaches the Context Source
    ${response}=    GET    url=${url}/csourceSubscriptions    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Empty    ${response.json()}
    Run Keyword And Expect Error    *    Wait For Request    ${3}

    # positive control: an unchained Subscription forwards its copy
    &{plain}=    Create Dictionary    Content-Type=application/json
    ${control}=    Set Variable
    ...    {"id": "${sub_id}:control", "type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": {"endpoint": {"uri": "http://original.subscriber.example:9998/notify"}}}
    ${response}=    POST    url=${url}/subscriptions    data=${control}    headers=${plain}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    Wait For Request    ${15}
    ${method}=    Get Request Method
    ${path}=    Get Request Url
    Should Be Equal    ${method}    POST
    Should Contain    ${path}    /ngsi-ld/v1/subscriptions
    ${body}=    Get Request Body
    ${body}=    Evaluate    $body.decode('utf-8') if isinstance($body, bytes) else $body
    Should Not Contain    ${body}    ${sub_id}"    the looped Subscription's copy must never leave the broker
    Reply By    201
    ${response}=    DELETE    url=${url}/subscriptions/${sub_id}:control    expected_status=any
    Check Response Status Code    204    ${response.status_code}


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

Get Broker Alias
    [Documentation]    Table 5.2.40-1: the broker's own contextSourceAlias,
    ...    as a peer would retrieve it before registering it.
    ${response}=    GET    url=${url}/info/sourceIdentity    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    RETURN    ${response.json()}[contextSourceAlias]

Clean Up
    ${response}=    DELETE    url=${url}/subscriptions/${sub_id}    expected_status=any
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
