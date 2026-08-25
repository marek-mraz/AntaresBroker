*** Settings ***
Documentation       Check multi-tenant behaviour (CIM 009 clause 4.14): "any information
...                 related to one Tenant ... [is] only visible to users of the same
...                 Tenant"; operations without a Tenant target the default Tenant; the
...                 NGSILD-Tenant header present in a request is echoed in the response
...                 (6.3.14); and for distributed operations "If no Tenant information is
...                 present in the Context Source Registration, no Tenant information is
...                 to be used and thus the default Tenant is targeted on the registered
...                 Context Source" — the requesting Tenant never flows through.
...
...                 Antares extension TP — the official suite runs tenant-blind except
...                 for the identity TP 061_01.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Suite Teardown      Cleanup Tenant Entities


*** Variables ***
${tenant_a}=                            tp414a
${tenant_b}=                            tp414b
${stub_entity_filename}=                vehicle-speed-attribute.json
${registration_payload_file_path}=      csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
414_01_01 Tenant Information Is Isolated And The Header Is Echoed
    [Documentation]    4.14: information of one Tenant is "only visible to users of the
    ...    same Tenant"; the default Tenant (no header) must NOT see it. 6.3.14: a
    ...    request-side NGSILD-Tenant header "shall also be present in HTTP response".
    [Tags]    common-behaviours    4_14    6_3_14    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity In Tenant    ${entity_id}    ${tenant_a}
    Check Response Status Code    201    ${response.status_code}
    ${echo}=    Evaluate    $response.headers.get('NGSILD-Tenant')
    Should Be Equal    ${echo}    ${tenant_a}
    ${response}=    Retrieve Entity In Tenant    ${entity_id}    ${tenant_a}
    Check Response Status Code    200    ${response.status_code}
    ${echo}=    Evaluate    $response.headers.get('NGSILD-Tenant')
    Should Be Equal    ${echo}    ${tenant_a}
    # the default Tenant must not see tenant_a's entity
    ${response}=    Retrieve Entity In Tenant    ${entity_id}    ${EMPTY}
    Check Response Status Code    404    ${response.status_code}

414_01_02 Another Tenant Cannot See The Entity
    [Documentation]    4.14: operations "never have any effect on the information of
    ...    other Tenants" — an existing second Tenant probing the first Tenant's id
    ...    gets 404
    [Tags]    common-behaviours    4_14    since_v1.9.1
    ${seed_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${seed_id}
    ${response}=    Create Entity In Tenant    ${seed_id}    ${tenant_b}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity In Tenant    ${entity_id}    ${tenant_b}
    Check Response Status Code    404    ${response.status_code}

414_01_03 Tenantless Registration Targets The Peer Default Tenant
    [Documentation]    4.14: "If no Tenant information is present in the Context Source
    ...    Registration, no Tenant information is to be used" — the forward triggered
    ...    from tenant_a must arrive at the registered source WITHOUT an NGSILD-Tenant
    ...    header
    [Tags]    common-behaviours    4_14    dist-ops    since_v1.9.1
    [Teardown]    Delete Registration In Tenant And Stop Mock
    ${fed_entity_id}=    Generate Random Vehicle Entity Id
    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${fed_entity_id}
    ${response}=    Create Registration In Tenant    ${registration_payload}    ${tenant_a}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server
    ${entity_body}=    Load Entity    ${stub_entity_filename}    ${fed_entity_id}
    ${entity_body_json}=    Evaluate    json.dumps($entity_body)    json
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${fed_entity_id}    200    ${entity_body_json}
    ${response}=    Retrieve Entity In Tenant    ${fed_entity_id}    ${tenant_a}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request
    ${headers}=    Get Request Headers
    ${forwarded_tenant}=    Evaluate    $headers.get('NGSILD-Tenant')
    Should Be Equal    ${forwarded_tenant}    ${None}
    ...    msg=the requesting Tenant must not flow through a tenant-less registration


*** Keywords ***
Create Entity In Tenant
    [Arguments]    ${id}    ${tenant}
    ${entity_body}=    Load Entity    ${stub_entity_filename}    ${id}
    ${payload}=    Evaluate    json.dumps($entity_body)    json
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary
    ...    Content-Type=application/json
    ...    Link=${context_link}
    ...    NGSILD-Tenant=${tenant}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    data=${payload}
    ...    expected_status=any
    RETURN    ${response}

Retrieve Entity In Tenant
    [Arguments]    ${id}    ${tenant}
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Link=${context_link}
    IF    '${tenant}' != ''
        Set To Dictionary    ${headers}    NGSILD-Tenant=${tenant}
    END
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${id}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Create Registration In Tenant
    [Arguments]    ${payload}    ${tenant}
    ${payload_json}=    Evaluate    json.dumps($payload)    json
    &{headers}=    Create Dictionary
    ...    Content-Type=application/ld+json
    ...    NGSILD-Tenant=${tenant}
    ${response}=    POST
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    data=${payload_json}
    ...    expected_status=any
    RETURN    ${response}

Delete In Tenant
    [Arguments]    ${path}    ${tenant}
    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant}
    ${response}=    DELETE
    ...    url=${url}/${path}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Delete Registration In Tenant And Stop Mock
    Delete In Tenant
    ...    ${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}/${registration_id}
    ...    ${tenant_a}
    Stop Context Source Mock Server

Cleanup Tenant Entities
    Delete In Tenant    ${ENTITIES_ENDPOINT_PATH}${entity_id}    ${tenant_a}
    Delete In Tenant    ${ENTITIES_ENDPOINT_PATH}${seed_id}    ${tenant_b}
