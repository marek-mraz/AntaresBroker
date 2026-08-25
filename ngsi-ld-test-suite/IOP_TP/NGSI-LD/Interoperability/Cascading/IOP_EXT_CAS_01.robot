*** Settings ***
Documentation       Cascading distributed operations, loops and Via (Antares
...                 extension IOP TPs). 6.3.18: "Any Context Broker
...                 implementation passing a distributed operation request
...                 onward to another Context Source shall send an
...                 additional field value on the Via header field using its
...                 own unique Context Source hostAlias (see clause 5.2.40)
...                 as the pseudonym." 6.3.17: warning 199 "No response was
...                 received from the registration endpoint within the
...                 specified timeout period or a registration loop has been
...                 detected"; 508 Loop Detected for an exclusive/redirect
...                 registration "registered to redirect back on to the
...                 Context Broker". 4.3.6.4: Via "can be used to exclude
...                 duplicated sources"; avoid "cascades of an excessive
...                 lengths, duplicates or loops". 5.2.34 localOnly:
...                 "distributed operations associated to this Context
...                 Source Registration will act only on data held directly
...                 by the registered Context Source itself".

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
${b4_url}
${mock_host}    127.0.0.1
${mock_port}    8095


*** Test Cases ***
IOP_EXT_CAS_01_01 Forwarded Requests Carry The Via Chain In Hop Order
    [Documentation]    6.3.18 Table 6.3.18-2: every broker passing a
    ...    distributed request onward "shall send an additional field value
    ...    on the Via header field using its own unique Context Source
    ...    hostAlias (see clause 5.2.40) as the pseudonym" — after two hops
    ...    (B1 → B2 → mock CS) the mock sees both pseudonyms, in hop order.
    [Tags]    iop    iop-ext    6_3_18    5_2_40    since_v1.9.1
    Start Server    ${mock_host}    ${mock_port}
    Set Test Variable    ${server_started}    ${True}
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=${etype}    200    []
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2
    ...    http://${mock_host}:${mock_port}/ngsi-ld/v1    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}

    Wait For Request    ${10}
    ${hdrs}=    Get Request Headers
    ${via}=    Evaluate    $hdrs.get('Via', '')
    Should Contain    ${via}    antares1
    Should Contain    ${via}    antares2
    Should Be True    ${{ $via.index('antares1') < $via.index('antares2') }}
    Should Not Contain    ${via}    antares3

IOP_EXT_CAS_01_02 Exclusive Registration Looping Back Is 508 On Unsafe Methods
    [Documentation]    6.3.17: "In the case of an exclusive or redirect
    ...    registration, where all of the data is held outside of the
    ...    Context Broker and held in a single registered source, the
    ...    following errors shall be returned: 508 Loop Detected - if the
    ...    single registered source and tenant is registered to redirect
    ...    back on to the Context Broker."
    [Tags]    iop    iop-ext    6_3_17    4_3_6_4    since_v1.9.1
    ${info}=    Evaluate
    ...    [{"entities": [{"id": $entity_id, "type": $etype}], "propertyNames": ["speed"]}]
    ${endpoint}=    Broker Base Of    ${b1_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "exclusive", "information": $info, "endpoint": $endpoint, "operations": ["redirectionOps"]}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}

    ${fragment}=    Evaluate    {"speed": {"type": "Property", "value": 11}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Check Response Status Code    508    ${response.status_code}
    Should Not Contain    ${response.text}    "value": 11

IOP_EXT_CAS_01_03 Redirect Registration Looping Back Is 508 On Unsafe Methods
    [Documentation]    6.3.17: 508 Loop Detected for a redirect registration
    ...    whose single registered source loops back on to the Context
    ...    Broker itself.
    [Tags]    iop    iop-ext    6_3_17    4_3_6_4    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"id": $entity_id, "type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b1_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $endpoint, "operations": ["redirectionOps"]}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}

    ${response}=    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    508    ${response.status_code}

IOP_EXT_CAS_01_04 Inclusive Loop Serves Data With Warning 199, Not 508
    [Documentation]    6.3.17: 508 is reserved for exclusive/redirect
    ...    single-source loops; Table 6.3.17-1 code 199 covers "a
    ...    registration loop has been detected". An inclusive B1→B2→B1 loop
    ...    still serves the union of both brokers' data with a 199 warning.
    [Tags]    iop    iop-ext    6_3_17    4_3_6_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2    ${b1_url}    ${etype}
    ${e1}=    Simple Vehicle Entity    ${entity_id}-b1    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e1}
    ${e2}=    Simple Vehicle Entity    ${entity_id}-b2    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${e2}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    Should Contain    ${response.text}    ${entity_id}-b1
    Should Contain    ${response.text}    ${entity_id}-b2
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    199

IOP_EXT_CAS_01_05 Auxiliary Loop Serves Data With Warning 199
    [Documentation]    6.3.17 Table 6.3.17-1 code 199 ("a registration loop
    ...    has been detected") + 4.3.6.2: auxiliary registrations are
    ...    consumption-only and never conflict — a self-looping auxiliary
    ...    registration degrades to local data with a warning, never 508.
    [Tags]    iop    iop-ext    6_3_17    4_3_6_2    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b1_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "auxiliary", "information": $info, "endpoint": $endpoint, "operations": ["queryEntity", "retrieveEntity"]}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    ${e1}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e1}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    ${warning}=    Evaluate    $response.headers.get('NGSILD-Warning', '')
    Should Contain    ${warning}    199

IOP_EXT_CAS_01_06 Three-Broker Chain Delivers A Remote Entity Exactly Once
    [Documentation]    4.3.6.4: cascading distributed operations must avoid
    ...    "cascades of an excessive lengths, duplicates or loops" — an
    ...    entity held only by B3 reaches B1's client through B2 exactly
    ...    once.
    [Tags]    iop    iop-ext    4_3_6_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    3
    Create Entity At Broker    ${b3_url}    ${e}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Be True    ${{ $response.text.count($entity_id) == 1 }}

IOP_EXT_CAS_01_07 Diamond Topology Yields The Entity Once In The Union
    [Documentation]    4.3.6.4: duplicates from parallel cascade paths
    ...    (B1→B2→B4 and B1→B3→B4) are excluded — B4's entity appears once.
    [Tags]    iop    iop-ext    4_3_6_4    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}-3    ${b3_url}    ${etype}
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2    ${b4_url}    ${etype}
    Register Broker As Context Source    ${b3_url}    ${registration_id}-4    ${b4_url}    ${etype}
    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    4
    Create Entity At Broker    ${b4_url}    ${e}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    Should Be True    ${{ $response.text.count($entity_id) == 1 }}

IOP_EXT_CAS_01_08 localOnly Stops The Cascade At The First Hop
    [Documentation]    5.2.34 localOnly: "distributed operations associated
    ...    to this Context Source Registration will act only on data held
    ...    directly by the registered Context Source itself (see clause
    ...    4.3.6.4)" — B2 answers from its own storage; B3 behind B2 is
    ...    never reached.
    [Tags]    iop    iop-ext    5_2_34    4_3_6_4    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint, "management": {"localOnly": True}}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    Register Broker As Context Source    ${b2_url}    ${registration_id}-2    ${b3_url}    ${etype}
    ${e2}=    Simple Vehicle Entity    ${entity_id}-b2    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${e2}
    ${e3}=    Simple Vehicle Entity    ${entity_id}-b3    ${etype}    3
    Create Entity At Broker    ${b3_url}    ${e3}

    ${via_b1}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${via_b1.status_code}
    Should Contain    ${via_b1.text}    ${entity_id}-b2
    Should Not Contain    ${via_b1.text}    ${entity_id}-b3
    ${via_b2}=    Query Entities Via Broker    ${b2_url}    type=${etype}
    Check Response Status Code    200    ${via_b2.status_code}
    Should Contain    ${via_b2.text}    ${entity_id}-b3


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopCas${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopCas:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopcas-${suffix}
    Set Test Variable    ${server_started}    ${False}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}-3
    Delete Registration At Broker    ${b2_url}    ${registration_id}-2
    Delete Registration At Broker    ${b3_url}    ${registration_id}-4
    FOR    ${eid}    IN    ${entity_id}    ${entity_id}-b1    ${entity_id}-b2    ${entity_id}-b3
        Delete Entity Via Broker    ${b1_url}    ${eid}
        Delete Entity Via Broker    ${b2_url}    ${eid}
        Delete Entity Via Broker    ${b3_url}    ${eid}
        Delete Entity Via Broker    ${b4_url}    ${eid}
    END
    IF    ${server_started}
        Stop Server
    END
