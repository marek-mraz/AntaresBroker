*** Settings ***
Documentation       Verify 5.5.10 Multi-Tenant Behaviour.
...
...                 5.5.10: creates implicitly create the Tenant; "All other
...                 NGSI-LD operations … that target a non-existing Tenant
...                 should raise an error of type NonexistentTenant"; the
...                 Tenant applied by an operation shall be provided in the
...                 response (6.3.14 header echo — errors included).
...
...                 Antares extension TP — the official suite never probes a
...                 non-existing tenant.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${ERROR_TYPE_NONEXISTENT_TENANT}=    https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant


*** Test Cases ***
5510_01_01 Non-Create Operations On An Unknown Tenant Are NonexistentTenant
    [Documentation]    5.5.10: query/retrieve on a never-created tenant →
    ...    404 NonexistentTenant, with the tenant echoed on the response;
    ...    after an implicit create the same query succeeds.
    [Tags]    common-behaviours    5_5_10    6_3_14    since_v1.9.1
    ${tenant}=    Evaluate    'tp5510' + str(random.randint(10000, 99999))    modules=random
    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}?type=Building
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_NONEXISTENT_TENANT}
    Should Be Equal    ${response.headers['NGSILD-Tenant']}    ${tenant}
    # implicit creation via Create Entity (5.6.1)
    ${entity_id}=    Generate Random Vehicle Entity Id
    &{post_headers}=    Create Dictionary    NGSILD-Tenant=${tenant}    Content-Type=application/json
    ${payload}=    Evaluate    {"id": $entity_id, "type": "Building"}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${post_headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    Should Be Equal    ${response.headers['NGSILD-Tenant']}    ${tenant}
    # the tenant now exists — the very same query answers 200
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}?type=Building
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    NonexistentTenant
    ${ids}=    Evaluate    [e['id'] for e in $response.json()]
    Should Contain    ${ids}    ${entity_id}

5510_01_02 The Default Tenant Implicitly Exists
    [Documentation]    5.5.10: "If no Tenant is specified, the operation shall
    ...    apply to the implicitly existing default Tenant" — a tenant-less
    ...    query is never NonexistentTenant.
    [Tags]    common-behaviours    5_5_10    since_v1.9.1
    ${response}=    Query Entities    entity_types=Building
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    NonexistentTenant
