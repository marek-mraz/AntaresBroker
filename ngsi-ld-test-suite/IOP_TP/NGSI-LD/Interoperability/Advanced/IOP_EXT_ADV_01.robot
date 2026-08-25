*** Settings ***
Documentation       Three-broker ADVANCED interoperability edges (Antares
...                 extension IOP TPs): tenant forwarding (4.14/6.3.14),
...                 cascade control with localOnly (4.3.6.4/6.3.18), the
...                 two-hop cascade B1→B2→B3, registration loops
...                 (4.3.6.4/6.3.17) and EntityMap pagination over a
...                 federated set (5.5.9.3/5.14).

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${b3_url}


*** Test Cases ***
IOP_EXT_ADV_01_01 Registration Tenant Reaches Broker2's Tenant
    [Documentation]    4.14: "the Tenant information from the Context Source
    ...    Registration has to be used" — the entity lives in B2 under
    ...    tenant iopzvolen; B1's registration carries tenant=iopzvolen, so
    ...    a DEFAULT-tenant retrieve via B1 serves it. Control: B2's own
    ...    default tenant does not know the entity.
    [Tags]    iop    iop-ext    4_14    6_3_14    since_v1.9.1
    ${tenant}=    Set Variable    iop${suffix}
    Set Test Variable    ${used_tenant}    ${tenant}
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    reg_tenant=${tenant}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}    tenant=${tenant}

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    404    ${response.status_code}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    42

IOP_EXT_ADV_01_02 localOnly Stops The Cascade At Broker2
    [Documentation]    4.3.6.4/6.3.18: B3's entity is reachable from B2 via
    ...    B2's own registration, but B1's registration to B2 carries
    ...    localOnly=true — so a query via B1 sees B2's own data and must
    ...    NOT see B3's.
    [Tags]    iop    iop-ext    4_3_6_4    6_3_18    since_v1.9.1
    Register Broker As Context Source    ${b2_url}    ${registration_id}-hop    ${b3_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    local_only=${True}
    ${e2}=    Simple Vehicle Entity    ${entity_id}-b2    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${e2}
    ${e3}=    Simple Vehicle Entity    ${entity_id}-b3    ${etype}    3
    Create Entity At Broker    ${b3_url}    ${e3}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-b2
    Should Not Contain    ${response.text}    ${entity_id}-b3

IOP_EXT_ADV_01_03 Without localOnly The Cascade Reaches Broker3
    [Documentation]    4.3.6.4 control case: the same topology WITHOUT
    ...    localOnly — B3's entity flows B3→B2→B1 across two hops.
    [Tags]    iop    iop-ext    4_3_6_4    since_v1.9.1
    Register Broker As Context Source    ${b2_url}    ${registration_id}-hop    ${b3_url}    ${etype}
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${e3}=    Simple Vehicle Entity    ${entity_id}-b3    ${etype}    3
    Create Entity At Broker    ${b3_url}    ${e3}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-b3

IOP_EXT_ADV_01_04 A Registration Loop Terminates And Deduplicates
    [Documentation]    4.3.6.4/6.3.17: B1 registers B2 and B2 registers B1
    ...    (a loop). A query via B1 must terminate (no cascade of excessive
    ...    length), answer 200, and return B1's entity exactly ONCE.
    [Tags]    iop    iop-ext    4_3_6_4    6_3_17    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    Register Broker As Context Source    ${b2_url}    ${registration_id}-back    ${b1_url}    ${etype}
    ${e1}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${e1}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    ${occurrences}=    Evaluate    $response.text.count('"' + $entity_id + '"')
    Should Be Equal As Integers    ${occurrences}    1

IOP_EXT_ADV_01_05 EntityMap Pages Walk A Federated Set
    [Documentation]    5.5.9.3/5.14 over 4.3.6: two entities in B1 and two
    ...    in B2; an EntityMap created via B1 fixes the union, and paging
    ...    with limit=3 serves all four across two full-to-the-maximum
    ...    pages.
    [Tags]    iop    iop-ext    5_5_9_3    5_14    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    FOR    ${i}    IN RANGE    2
        ${e}=    Simple Vehicle Entity    ${entity_id}-l${i}    ${etype}    ${i}
        Create Entity At Broker    ${b1_url}    ${e}
        ${e}=    Simple Vehicle Entity    ${entity_id}-r${i}    ${etype}    ${i}
        Create Entity At Broker    ${b2_url}    ${e}
    END

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}    entityMap=true
    Check Response Status Code    201    ${response.status_code}
    ${map_ref}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap

    ${page1}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}    limit=3
    Check Response Status Code    200    ${page1.status_code}
    Length Should Be    ${page1.json()}    3
    ${page2}=    Query Entities Via Broker    ${b1_url}    map_ref=${map_ref}    type=${etype}    limit=3    offset=3
    Check Response Status Code    200    ${page2.status_code}
    Length Should Be    ${page2.json()}    1
    ${union}=    Evaluate    {e["id"] for e in $page1.json()} | {e["id"] for e in $page2.json()}
    Length Should Be    ${union}    4


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}
    Set Test Variable    ${used_tenant}    ${EMPTY}

Cleanup Interop Fixtures
    FOR    ${tail}    IN    ${EMPTY}    -b2    -b3    -l0    -l1    -r0    -r1
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b3_url}    ${entity_id}${tail}
    END
    IF    '${used_tenant}' != ''
        Delete Entity Via Broker    ${b2_url}    ${entity_id}    tenant=${used_tenant}
    END
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    Delete Registration At Broker    ${b2_url}    ${registration_id}-hop
    Delete Registration At Broker    ${b2_url}    ${registration_id}-back
