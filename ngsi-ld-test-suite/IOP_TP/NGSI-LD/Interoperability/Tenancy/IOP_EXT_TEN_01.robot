*** Settings ***
Documentation       Tenancy across brokers (Antares extension IOP TPs).
...                 4.14: "When contacting the respective Context Sources,
...                 the Tenant information from the Context Source
...                 Registration has to be used. If no Tenant information
...                 is present in the Context Source Registration, no
...                 Tenant information is to be used and thus the default
...                 Tenant is targeted"; "any information related to one
...                 Tenant ... only visible to users of the same Tenant".
...                 6.3.14: "If the HTTP header NGSILD-Tenant is present in
...                 the HTTP request, it shall also be present in HTTP
...                 response. This also applies to HTTP notifications sent
...                 as a result of subscriptions with an NGSILD-Tenant HTTP
...                 header."

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
IOP_EXT_TEN_01_01 Without A Registration Tenant The Forward Targets The Default Tenant
    [Documentation]    4.14: "If no Tenant information is present in the
    ...    Context Source Registration, no Tenant information is to be used
    ...    and thus the default Tenant is targeted" — the CLIENT's tenant
    ...    header never propagates to the Context Source.
    [Tags]    iop    iop-ext    4_14    6_3_14    since_v1.9.1
    Register Tenant Scoped    ${tenant_a}    ${EMPTY}
    ${def}=    Simple Vehicle Entity    ${entity_id}-def    ${etype}    1
    Create Entity At Broker    ${b2_url}    ${def}
    ${ten}=    Simple Vehicle Entity    ${entity_id}-ten    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${ten}    tenant=${tenant_a}

    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant_a}
    ${response}=    GET    url=${b1_url}/entities    params=type=${etype}    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-def
    Should Not Contain    ${response.text}    ${entity_id}-ten

IOP_EXT_TEN_01_02 The Registration Tenant Overrides The Client Tenant
    [Documentation]    4.14: "the Tenant information from the Context
    ...    Source Registration has to be used" — a client under tenant A
    ...    querying through a registration carrying tenant B reads B2's
    ...    tenant-B data, never its tenant-A data.
    [Tags]    iop    iop-ext    4_14    5_2_9    since_v1.9.1
    Register Tenant Scoped    ${tenant_a}    ${tenant_b}
    ${in_b}=    Simple Vehicle Entity    ${entity_id}-b    ${etype}    2
    Create Entity At Broker    ${b2_url}    ${in_b}    tenant=${tenant_b}
    ${in_a}=    Simple Vehicle Entity    ${entity_id}-a    ${etype}    3
    Create Entity At Broker    ${b2_url}    ${in_a}    tenant=${tenant_a}

    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant_a}
    ${response}=    GET    url=${b1_url}/entities    params=type=${etype}    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-b
    Should Not Contain    ${response.text}    ${entity_id}-a

IOP_EXT_TEN_01_03 A Remote Nonexistent Tenant Degrades Silently To Local Data
    [Documentation]    4.14 + 6.3.17: the remote broker answers
    ...    NonexistentTenant (404) for the registration's unknown tenant —
    ...    "a registration endpoint responding with no data and the HTTP
    ...    status code 404 - Not Found should not be considered as abnormal
    ...    behaviour", so the local data is served without a client-visible
    ...    error.
    [Tags]    iop    iop-ext    4_14    6_3_17    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint, "tenant": "ghost-" + $suffix}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}
    ${local}=    Simple Vehicle Entity    ${entity_id}-l    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${local}

    ${response}=    Query Entities Via Broker    ${b1_url}    type=${etype}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}-l
    Should Not Contain    ${response.text}    NonexistentTenant

IOP_EXT_TEN_01_04 A Redirected Write Lands In The Registration's Tenant Only
    [Documentation]    4.14/5.2.9 tenant: the forwarded create carries the
    ...    registration's tenant — the entity exists in B2's registration
    ...    tenant and is absent from B2's default tenant.
    [Tags]    iop    iop-ext    4_14    5_2_9    since_v1.9.1
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "mode": "redirect", "information": $info, "endpoint": $endpoint, "operations": ["redirectionOps"], "tenant": $tenant_b}
    ${created}=    Post Registration At Broker    ${b1_url}    ${reg}
    Check Response Status Code    201    ${created.status_code}

    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    5
    ${response}=    Create Entity At Broker    ${b1_url}    ${e}
    Check Response Status Code    201    ${response.status_code}

    ${in_b}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    tenant=${tenant_b}
    Check Response Status Code    200    ${in_b.status_code}
    ${in_def}=    Get Entity Via Broker    ${b2_url}    ${entity_id}
    Check Response Status Code    404    ${in_def.status_code}

IOP_EXT_TEN_01_05 The Same Entity Id In Different Tenants Never Merges
    [Documentation]    4.14: operations "only apply to the information of
    ...    the specified Tenant in isolation and never have any effect on
    ...    the information of other Tenants" — B1's tenant-A entity is
    ...    served untouched even though B2's default tenant holds a richer
    ...    entity under the same id (the registration lives in the default
    ...    tenant, so tenant A has no registrations at all).
    [Tags]    iop    iop-ext    4_14    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ${remote}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 99}, "brandName": {"type": "Property", "value": "Ghost"}}
    Create Entity At Broker    ${b2_url}    ${remote}
    ${local}=    Simple Vehicle Entity    ${entity_id}    ${etype}    1
    Create Entity At Broker    ${b1_url}    ${local}    tenant=${tenant_a}

    ${in_a}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    tenant=${tenant_a}
    Check Response Status Code    200    ${in_a.status_code}
    Should Be Equal As Integers    ${in_a.json()['speed']['value']}    1
    Should Not Contain    ${in_a.text}    brandName
    ${in_def}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${in_def.status_code}
    Should Contain    ${in_def.text}    brandName

IOP_EXT_TEN_01_06 The Notification Chain Preserves The Origin Tenant
    [Documentation]    6.3.14: "This also applies to HTTP notifications
    ...    sent as a result of subscriptions with an NGSILD-Tenant HTTP
    ...    header" — the subscriber of a tenant-A subscription receives the
    ...    notification with NGSILD-Tenant: A.
    [Tags]    iop    iop-ext    6_3_14    5_8_1    since_v1.9.1
    Register Tenant Scoped    ${tenant_a}    ${tenant_a}
    Start Server    ${notify_host}    ${notify_port}
    Set Test Variable    ${server_started}    ${True}
    ${sub}=    Evaluate
    ...    {"id": $subscription_id, "type": "Subscription", "entities": [{"type": $etype}], "notification": {"endpoint": {"uri": "http://" + $notify_host + ":" + str($notify_port) + "/tnotify", "accept": "application/json"}}}
    &{headers}=    Create Dictionary    Content-Type=application/json    NGSILD-Tenant=${tenant_a}
    ${response}=    POST    url=${b1_url}/subscriptions    json=${sub}    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    Sleep    1s

    ${e}=    Simple Vehicle Entity    ${entity_id}    ${etype}    64
    Create Entity At Broker    ${b2_url}    ${e}    tenant=${tenant_a}

    Wait For Request    ${30}
    ${hdrs}=    Get Request Headers
    ${body}=    Get Request Body
    Reply By    200
    Should Be Equal    ${{ $hdrs.get('NGSILD-Tenant', '') }}    ${tenant_a}
    Should Contain    ${body.decode('utf-8')}    ${entity_id}
    Should Not Contain    ${body.decode('utf-8')}    ghost


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${suffix}
    Set Test Variable    ${etype}    IopTen${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopTen:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopten-${suffix}
    Set Test Variable    ${subscription_id}    urn:ngsi-ld:Subscription:iopten-${suffix}
    Set Test Variable    ${tenant_a}    iopa${suffix}
    Set Test Variable    ${tenant_b}    iopb${suffix}
    Set Test Variable    ${server_started}    ${False}

Register Tenant Scoped
    [Documentation]    Register B2 as a source under client tenant
    ...    ${at_tenant} at B1; ${reg_tenant} non-empty adds the 5.2.9
    ...    tenant member.
    [Arguments]    ${at_tenant}    ${reg_tenant}
    ${info}=    Evaluate    [{"entities": [{"type": $etype}]}]
    ${endpoint}=    Broker Base Of    ${b2_url}
    ${reg}=    Evaluate
    ...    {"id": $registration_id, "type": "ContextSourceRegistration", "information": $info, "endpoint": $endpoint}
    IF    '${reg_tenant}' != ''
        Evaluate    $reg.update({"tenant": $reg_tenant})
    END
    &{headers}=    Create Dictionary    Content-Type=application/json
    IF    '${at_tenant}' != ''
        Set To Dictionary    ${headers}    NGSILD-Tenant=${at_tenant}
    END
    ${response}=    POST    url=${b1_url}/csourceRegistrations    json=${reg}    headers=${headers}
    ...    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    201
    RETURN    ${response}

Cleanup Interop Fixtures
    Delete Registration At Broker    ${b1_url}    ${registration_id}
    &{ha}=    Create Dictionary    NGSILD-Tenant=${tenant_a}
    DELETE    url=${b1_url}/csourceRegistrations/${registration_id}    headers=${ha}    expected_status=any
    DELETE    url=${b1_url}/subscriptions/${subscription_id}    headers=${ha}    expected_status=any
    FOR    ${tail}    IN    ${EMPTY}    -def    -ten    -a    -b    -l
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}
        Delete Entity Via Broker    ${b1_url}    ${entity_id}${tail}    tenant=${tenant_a}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}    tenant=${tenant_a}
        Delete Entity Via Broker    ${b2_url}    ${entity_id}${tail}    tenant=${tenant_b}
    END
    IF    ${server_started}
        Stop Server
    END
