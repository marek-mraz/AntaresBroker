*** Settings ***
Documentation       Verify the payload of Retrieve Context Source Identity Information.
...
...                 5.15.1.4: "Return a JSON-LD object representing the identity of the
...                 Context Source itself as mandated by clause 5.2.40."
...                 Table 5.2.40-1 defines the members, and three of them are mandatory at
...                 cardinality 1: contextSourceAlias (a unique id used to identify
...                 loops), contextSourceUptime (an ISO 8601 duration) and
...                 contextSourceTimeAt (a 4.6.3 DateTime). contextSourceExtras is 0..1.
...
...                 Antares extension TP — clause 5.15 has no official TPs, which is why
...                 a payload using non-spec member names (hostAlias/uptime, neither of
...                 which is a core-context term) went unnoticed.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Variables ***
${SOURCE_IDENTITY_ENDPOINT_PATH}        info/sourceIdentity


*** Test Cases ***
061_01_01 Retrieve Context Source Identity Information
    [Documentation]    The response carries type ContextSourceIdentity and a URI id
    [Tags]    common-behaviours    5_15_1    5_2_40    6_33    since_v1.9.1

    ${response}=    Retrieve Source Identity

    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    Should Be Equal    ${body["type"]}    ContextSourceIdentity
    Should Start With    ${body["id"]}    urn:    the id shall be a valid URI

061_01_02 Context Source Identity Carries The Mandatory Members
    [Documentation]    contextSourceAlias, contextSourceUptime and contextSourceTimeAt are
    ...    all cardinality 1 in Table 5.2.40-1
    [Tags]    common-behaviours    5_15_1    5_2_40    since_v1.9.1

    ${response}=    Retrieve Source Identity
    ${body}=    Set Variable    ${response.json()}

    Dictionary Should Contain Key    ${body}    contextSourceAlias
    Dictionary Should Contain Key    ${body}    contextSourceUptime
    Dictionary Should Contain Key    ${body}    contextSourceTimeAt
    Should Not Be Empty    ${body["contextSourceAlias"]}

061_01_03 Context Source Uptime Is An ISO 8601 Duration
    [Documentation]    Table 5.2.40-1: "String representing a duration in ISO 8601 format"
    [Tags]    common-behaviours    5_15_1    5_2_40    since_v1.9.1

    ${response}=    Retrieve Source Identity
    ${uptime}=    Set Variable    ${response.json()["contextSourceUptime"]}

    Should Match Regexp
    ...    ${uptime}
    ...    ^P(?!$)(\\d+Y)?(\\d+M)?(\\d+W)?(\\d+D)?(T(?!$)(\\d+H)?(\\d+M)?(\\d+(\\.\\d+)?S)?)?$
    ...    contextSourceUptime must be an ISO 8601 duration, got "${uptime}"

061_01_04 Context Source TimeAt Is A DateTime In UTC
    [Documentation]    Table 5.2.40-1 types it as DateTime (clause 4.6.3), and 4.6.3
    ...    requires the trailing component to "always be equal to the character Z"
    [Tags]    common-behaviours    5_15_1    5_2_40    4_6_3    since_v1.9.1

    ${response}=    Retrieve Source Identity
    ${time_at}=    Set Variable    ${response.json()["contextSourceTimeAt"]}

    Should Match Regexp
    ...    ${time_at}
    ...    ^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(\\.\\d{1,6})?Z$
    ...    contextSourceTimeAt must be a 4.6.3 UTC DateTime, got "${time_at}"


061_01_05 Context Source Alias Identifies A Specific Tenant
    [Documentation]    Table 5.2.40-1: "In the multi-tenancy use case (see clause 4.14),
    ...    this id shall be identifying a specific Tenant within a registered Context
    ...    Source." One alias for every tenant of a broker makes its tenants
    ...    indistinguishable in a Via chain, so cross-tenant federation within one
    ...    Context Source is misread as a loop (6.3.17/6.3.18).
    [Tags]    common-behaviours    5_15_1    5_2_40    4_14    6_3_18    since_v1.9.1

    ${default}=    Retrieve Source Identity
    ${tenant_a}=    Retrieve Source Identity    tenant=urnalias-a
    ${tenant_b}=    Retrieve Source Identity    tenant=urnalias-b

    Should Not Be Equal
    ...    ${tenant_a.json()["contextSourceAlias"]}
    ...    ${default.json()["contextSourceAlias"]}
    ...    a tenant's alias shall identify that tenant, not just the Context Source
    Should Not Be Equal
    ...    ${tenant_a.json()["contextSourceAlias"]}
    ...    ${tenant_b.json()["contextSourceAlias"]}
    ...    two tenants of one Context Source shall not share an alias

    # …and it is stable: the same tenant retrieves the same alias, which is what
    # a peer stores as contextSourceAlias in its registration (Table 5.2.9-1).
    ${again}=    Retrieve Source Identity    tenant=urnalias-a
    Should Be Equal
    ...    ${again.json()["contextSourceAlias"]}
    ...    ${tenant_a.json()["contextSourceAlias"]}


*** Keywords ***
Retrieve Source Identity
    [Arguments]    ${tenant}=${EMPTY}
    &{headers}=    Create Dictionary    Accept=application/json
    IF    '${tenant}' != ''
        Set To Dictionary    ${headers}    NGSILD-Tenant=${tenant}
    END
    ${response}=    GET
    ...    url=${url}/${SOURCE_IDENTITY_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Retrieve Source Identity
    RETURN    ${response}
