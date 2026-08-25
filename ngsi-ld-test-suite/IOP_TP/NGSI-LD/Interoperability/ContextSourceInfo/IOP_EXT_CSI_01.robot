*** Settings ***
Documentation       contextSourceInfo (Antares extension IOP TPs). 4.3.6.5:
...                 the contextSourceInfo array carries "whatever extra
...                 information the Context Broker shall convey when
...                 contacting the Context Source". 6.3.19 (HTTP binding):
...                 "a new custom HTTP header is added for each member named
...                 'key' ... The content of each custom header shall be set
...                 equal to the content of the corresponding 'value'
...                 member". 4.3.6.6: the keys "accept" and "jsonldContext"
...                 are pre-processed rather than forwarded verbatim — for
...                 "jsonldContext" "the URL value is placed in an HTTP Link
...                 Header" (6.3.19) and a compaction is applied, for
...                 "accept" the response "shall be returned in this defined
...                 format and if necessary, the Context Broker shall be
...                 responsible for converting this to the desired content
...                 type when aggregating".

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections
Library             RequestsLibrary
Library             HttpCtrl.Server

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${mock_host}    127.0.0.1
${mock_port}    8096


*** Test Cases ***
IOP_EXT_CSI_01_01 A contextSourceInfo Pair Becomes A Forward Header
    [Documentation]    4.3.6.5/6.3.19: "a new custom HTTP header is added
    ...    for each member named 'key' ... set equal to the content of the
    ...    corresponding 'value' member" — the pair arrives at the Context
    ...    Source verbatim.
    [Tags]    iop    iop-ext    4_3_6_5    6_3_19    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "X-Custom-Token", "value": "s3cr3t"}]
    Register Mock With Csi    ${csi}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request    ${10}
    ${hdrs}=    Get Request Headers
    ${got}=    Evaluate    $hdrs.get('X-Custom-Token', '')
    Should Be Equal    ${got}    s3cr3t

IOP_EXT_CSI_01_02 Multiple contextSourceInfo Pairs All Arrive Verbatim
    [Documentation]    4.3.6.5/6.3.19: one custom header per pair, values
    ...    verbatim.
    [Tags]    iop    iop-ext    4_3_6_5    6_3_19    since_v1.9.1
    ${csi}=    Evaluate
    ...    [{"key": "X-First", "value": "alpha"}, {"key": "X-Second", "value": "beta"}]
    Register Mock With Csi    ${csi}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request    ${10}
    ${hdrs}=    Get Request Headers
    Should Be Equal    ${{ $hdrs.get('X-First', '') }}    alpha
    Should Be Equal    ${{ $hdrs.get('X-Second', '') }}    beta

IOP_EXT_CSI_01_03 jsonldContext Is Pre-Processed, Never A Literal Header
    [Documentation]    4.3.6.6: "If the key 'jsonldContext' is defined ...
    ...    the Context Broker shall apply a compaction operation ... prior
    ...    to distributing the request." 6.3.19: "the URL value is placed
    ...    in an HTTP Link Header" — no literal jsonldContext header may
    ...    reach the Context Source.
    [Tags]    iop    iop-ext    4_3_6_6    6_3_19    since_v1.9.1
    ${csi}=    Evaluate
    ...    [{"key": "jsonldContext", "value": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"}]
    Register Mock With Csi    ${csi}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request    ${10}
    ${hdrs}=    Get Request Headers
    Should Be Equal    ${{ $hdrs.get('jsonldContext', '') }}    ${EMPTY}
    ${link}=    Evaluate    $hdrs.get('Link', '')
    Should Contain    ${link}    ngsi-ld-core-context

IOP_EXT_CSI_01_04 Authorization Pair Reaches The Source But Never The Client
    [Documentation]    4.3.6.5: conveying "Authorization material" is the
    ...    clause's own example — it must reach the Context Source, and it
    ...    must NOT leak back into the client-facing response headers.
    [Tags]    iop    iop-ext    4_3_6_5    6_3_19    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "Authorization", "value": "Bearer iop-secret"}]
    Register Mock With Csi    ${csi}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    Authorization
    Should Not Contain    ${response.text}    iop-secret
    Wait For Request    ${10}
    ${hdrs}=    Get Request Headers
    Should Be Equal    ${{ $hdrs.get('Authorization', '') }}    Bearer iop-secret

IOP_EXT_CSI_01_05 No contextSourceInfo Means No Extra Headers
    [Documentation]    4.3.6.5 negative baseline: without contextSourceInfo
    ...    the forward carries no custom pair headers.
    [Tags]    iop    iop-ext    4_3_6_5    since_v1.9.1
    Register Mock With Csi    ${None}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request    ${10}
    ${hdrs}=    Get Request Headers
    Should Be Equal    ${{ $hdrs.get('X-Custom-Token', '') }}    ${EMPTY}
    Should Be Equal    ${{ $hdrs.get('Authorization', '') }}    ${EMPTY}

IOP_EXT_CSI_01_06 accept Key Steers The Forward Without Breaking Negotiation
    [Documentation]    4.3.6.6: "If the key 'accept' is defined ... the
    ...    response from the distributed endpoint shall be returned in this
    ...    defined format and if necessary, the Context Broker shall be
    ...    responsible for converting this to the desired content type when
    ...    aggregating responses to the initial request" — an
    ...    application/ld+json source answer still reaches the plain-JSON
    ...    client without an inline @context member.
    [Tags]    iop    iop-ext    4_3_6_6    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "accept", "value": "application/ld+json"}]
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 5}, "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"}
    ${body}=    Evaluate    __import__('json').dumps([$entity])
    Register Mock With Csi    ${csi}    body=${body}
    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request    ${10}
    ${hdrs}=    Get Request Headers
    Should Contain    ${{ $hdrs.get('Accept', '') }}    application/ld+json
    Should Contain    ${response.text}    ${entity_id}
    Should Contain    ${response.headers['Content-Type']}    application/json
    Should Not Contain    ${response.text}    @context


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopCsi${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopCsi:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopcsi-${suffix}
    Set Test Variable    ${server_started}    ${False}

Register Mock With Csi
    [Documentation]    Start the mock CS, stub the forwarded query, and
    ...    register it at B1 with the given contextSourceInfo array.
    [Arguments]    ${csi}    ${body}=[]
    Start Server    ${mock_host}    ${mock_port}
    Set Test Variable    ${server_started}    ${True}
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    ${body}
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": "http://" + $mock_host + ":" + str($mock_port)}
    IF    $csi is not None
        Evaluate    $reg.update({"contextSourceInfo": $csi})
    END
    ${response}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${response.status_code}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    IF    ${server_started}
        Stop Server
    END
