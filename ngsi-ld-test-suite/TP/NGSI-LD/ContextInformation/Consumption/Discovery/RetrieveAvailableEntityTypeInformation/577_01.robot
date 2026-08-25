*** Settings ***
Documentation       Verify the 5.7.7 error path and the tenant scope of the
...                 5.7.5 entity type list.
...
...                 5.7.7: the operation returns the information of "the
...                 specified entity type ... for which instances exist
...                 within the NGSI-LD system"; a type no entity instance
...                 uses is a ResourceNotFound (Table 6.3.2-1).
...                 5.7.11 answers discovery from the local datastore, which
...                 under 5.5.10 is the datastore of the Tenant the request
...                 applies to.
...
...                 Antares extension TP — the official 022/024 TPs only
...                 exercise the happy path on the default Tenant.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
577_01_01 An Entity Type No Instance Uses Is ResourceNotFound
    [Documentation]    5.7.7: only types "for which instances exist" are
    ...    described; anything else is 404 ResourceNotFound.
    [Tags]    ed-type    5_7_7    since_v1.9.1
    ${type}=    Evaluate    'NoSuchType' + str(random.randint(10000, 99999))    modules=random
    ${response}=    Retrieve Entity Type    type=${type}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}

577_01_02 An Undefined Query Parameter Is InvalidRequest
    [Documentation]    6.3.20: GET /types/{type} defines only "local"; any
    ...    other query parameter is rejected with 400 InvalidRequest.
    [Tags]    ed-type    5_7_7    6_3_20    since_v1.9.1
    ${response}=    GET
    ...    url=${url}/${ENTITIES_TYPES_ENDPOINT_PATH}/Building?details=true
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}

577_01_03 Entity Types Are Scoped To The Tenant Of The Request
    [Documentation]    5.7.11 + 5.5.10: the type list is folded from the
    ...    local datastore of the requesting Tenant — a type only another
    ...    Tenant's entities carry is neither listed nor retrievable.
    [Tags]    ed-type    ed-types    5_7_5    5_7_7    5_5_10    since_v1.9.1
    ${suffix}=    Evaluate    str(random.randint(10000, 99999))    modules=random
    ${tenant_a}=    Set Variable    tp577a${suffix}
    ${tenant_b}=    Set Variable    tp577b${suffix}
    ${type_a}=    Set Variable    TenantAType${suffix}
    ${id_a}=    Generate Random Building Entity Id
    ${id_b}=    Generate Random Building Entity Id
    Create Entity In Tenant    ${tenant_a}    ${id_a}    ${type_a}
    Create Entity In Tenant    ${tenant_b}    ${id_b}    Building
    # tenant A sees its own type
    ${response}=    Get In Tenant    ${tenant_a}    ${ENTITIES_TYPES_ENDPOINT_PATH}
    Check Response Status Code    200    ${response.status_code}
    ${types}=    Evaluate    $response.json()['typeList']
    Should Contain    ${types}    ${type_a}
    # tenant B does not
    ${response}=    Get In Tenant    ${tenant_b}    ${ENTITIES_TYPES_ENDPOINT_PATH}
    Check Response Status Code    200    ${response.status_code}
    ${types}=    Evaluate    $response.json()['typeList']
    Should Not Contain    ${types}    ${type_a}
    ${response}=    Get In Tenant    ${tenant_b}    ${ENTITIES_TYPES_ENDPOINT_PATH}/${type_a}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    [Teardown]    Delete Entities In Tenants    ${tenant_a}    ${id_a}    ${tenant_b}    ${id_b}


*** Keywords ***
Create Entity In Tenant
    [Arguments]    ${tenant}    ${entity_id}    ${entity_type}
    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant}    Content-Type=application/json
    ${payload}=    Evaluate    {"id": $entity_id, "type": $entity_type}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Get In Tenant
    [Arguments]    ${tenant}    ${path}
    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant}
    ${response}=    GET
    ...    url=${url}/${path}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Delete Entities In Tenants
    [Arguments]    ${tenant_a}    ${id_a}    ${tenant_b}    ${id_b}
    &{headers_a}=    Create Dictionary    NGSILD-Tenant=${tenant_a}
    DELETE    url=${url}/${ENTITIES_ENDPOINT_PATH}/${id_a}    headers=${headers_a}    expected_status=any
    &{headers_b}=    Create Dictionary    NGSILD-Tenant=${tenant_b}
    DELETE    url=${url}/${ENTITIES_ENDPOINT_PATH}/${id_b}    headers=${headers_b}    expected_status=any
