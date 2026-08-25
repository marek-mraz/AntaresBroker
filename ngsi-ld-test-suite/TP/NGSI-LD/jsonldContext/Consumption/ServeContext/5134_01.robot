*** Settings ***
Documentation       Verify that a Hosted @context is private to the Tenant that added it.
...
...                 5.13.2.4 stores the @context supplied by a requesting
...                 client and 5.13.4.4 serves that content back; 5.5.10 makes
...                 the Tenant the boundary an operation applies within, so a
...                 Hosted @context added under one Tenant shall not be served,
...                 listed or deleted through another Tenant. Cached @contexts
...                 are broker-fetched copies of public documents (5.13.1) and
...                 stay visible to every Tenant.
...
...                 Antares extension TP — the official suite never sends
...                 NGSILD-Tenant to the jsonldContexts resources.

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections
Library             String


*** Variables ***
${PRIVATE_TERM}=    TenantPrivateTerm


*** Test Cases ***
5134_01_01 A Hosted @context Is Not Served To Another Tenant
    [Documentation]    5.13.4.4 + 5.5.10: the term mappings added under one
    ...    Tenant are not served through another Tenant — the entry is as
    ...    absent as one that never existed (404).
    [Tags]    ctx-serve    5_13_4    5_5_10    since_v1.9.1
    ${tenant_a}=    Random Tenant
    ${tenant_b}=    Random Tenant
    ${local_id}=    Add A Hosted @context As Tenant    ${tenant_a}

    ${response}=    Serve A @context As Tenant    ${tenant_b}    ${local_id}    ${EMPTY}
    Check Response Status Code    404    ${response.status_code}
    Should Not Contain    ${response.text}    ${PRIVATE_TERM}

    # not even the metadata of the other Tenant's entry (5.13.3.5)
    ${response}=    Serve A @context As Tenant    ${tenant_b}    ${local_id}    true
    Check Response Status Code    404    ${response.status_code}

    # the owning Tenant is unaffected
    ${response}=    Serve A @context As Tenant    ${tenant_a}    ${local_id}    ${EMPTY}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${PRIVATE_TERM}

5134_01_02 A Hosted @context Is Not Listed To Another Tenant
    [Documentation]    5.13.3.4 + 5.5.10: the listing of another Tenant's
    ...    @contexts exposes neither its URLs nor its usage metadata.
    [Tags]    ctx-list    5_13_3    5_5_10    since_v1.9.1
    ${tenant_a}=    Random Tenant
    ${tenant_b}=    Random Tenant
    ${local_id}=    Add A Hosted @context As Tenant    ${tenant_a}

    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant_b}
    ${response}=    GET
    ...    url=${url}/${JSONLDCONTEXTS_ENDPOINT_PATH}?details=true
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${local_id}

    ${response}=    GET
    ...    url=${url}/${JSONLDCONTEXTS_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${local_id}

5134_01_03 A Hosted @context Is Not Deletable By Another Tenant
    [Documentation]    5.13.5.4 + 5.5.10: a delete issued under another Tenant
    ...    is a 404 and shall leave the @context in place.
    [Tags]    ctx-delete    5_13_5    5_5_10    since_v1.9.1
    ${tenant_a}=    Random Tenant
    ${tenant_b}=    Random Tenant
    ${local_id}=    Add A Hosted @context As Tenant    ${tenant_a}

    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant_b}
    ${response}=    DELETE
    ...    url=${url}/${JSONLDCONTEXTS_ENDPOINT_PATH}/${local_id}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    404    ${response.status_code}

    ${response}=    Serve A @context As Tenant    ${tenant_a}    ${local_id}    ${EMPTY}
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${PRIVATE_TERM}

    # the owning Tenant can still delete it (5.13.5.4)
    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant_a}
    ${response}=    DELETE
    ...    url=${url}/${JSONLDCONTEXTS_ENDPOINT_PATH}/${local_id}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}


*** Keywords ***
Random Tenant
    ${tenant}=    Evaluate    'tp5134' + str(random.randint(10000, 99999))    modules=random
    RETURN    ${tenant}

Add A Hosted @context As Tenant
    [Documentation]    5.13.2.4: add a @context under ${tenant}, returning its
    ...    local id (the last segment of the Location header).
    [Arguments]    ${tenant}
    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant}    Content-Type=application/json
    ${payload}=    Evaluate    {"@context": {"${PRIVATE_TERM}": "urn:ngsi-ld:attributes:${PRIVATE_TERM}"}}
    ${response}=    POST
    ...    url=${url}/${JSONLDCONTEXTS_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${location}=    Fetch Id From Response Location Header    ${response.headers}
    ${local_id}=    Fetch From Right    ${location}    /
    RETURN    ${local_id}

Serve A @context As Tenant
    [Documentation]    5.13.4.4: retrieve a @context under ${tenant}.
    [Arguments]    ${tenant}    ${local_id}    ${details}
    &{headers}=    Create Dictionary    NGSILD-Tenant=${tenant}
    IF    '${details}'!=''
        ${response}=    GET
        ...    url=${url}/${JSONLDCONTEXTS_ENDPOINT_PATH}/${local_id}?details=${details}
        ...    headers=${headers}
        ...    expected_status=any
    ELSE
        ${response}=    GET
        ...    url=${url}/${JSONLDCONTEXTS_ENDPOINT_PATH}/${local_id}
        ...    headers=${headers}
        ...    expected_status=any
    END
    RETURN    ${response}
