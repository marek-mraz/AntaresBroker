*** Settings ***
Documentation       Check that Purge Entities rejects a request whose only filters are
...                 Entity identifiers, or whose Attribute list / query names nothing but
...                 system Attributes.
...
...                 5.6.21.4: "At least one of the following input data shall be provided:
...                 a) selector of Entity Types; b) list of Attribute names, including at
...                 least one non-system Attribute; c) NGSI-LD Query, including at least
...                 one non-system Attribute; d) NGSI-LD GeoQuery; e) local scope. If none
...                 of the above is provided, then an error of type BadRequestData shall
...                 be raised (too wide query)." 5.6.21.3 adds: "it is not possible to
...                 purge a set of entities by only specifying desired Entity identifiers".
...                 Table 6.4.3.3-1 states the same constraint in the HTTP binding.
...
...                 Antares extension TP — the official 060_01 covers only the
...                 no-parameters-at-all case, which leaves `id`/`idPattern` untested.
...                 Regression guard: `DELETE /entities?idPattern=.*` deleted the tenant.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
060_06_01 Purge Entities With Only An Entity Id
    [Documentation]    id alone is legal input data but is not a qualifying filter (5.6.21.4)
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities    id=${entity_id}    context=${ngsild_test_suite_context}

    Purge Was Refused As Too Wide    ${response}
    Entity Still Exists

060_06_02 Purge Entities With Only An Id Pattern
    [Documentation]    idPattern alone is not a qualifying filter — a match-all pattern
    ...    must not become an unfiltered tenant-wide delete
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities    idPattern=.*    context=${ngsild_test_suite_context}

    Purge Was Refused As Too Wide    ${response}
    Entity Still Exists

060_06_03 Purge Entities With Only System Attributes In Attrs
    [Documentation]    5.6.21.4 b) requires the Attribute list to include at least one
    ...    non-system Attribute; createdAt is system-generated
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities    attrs=createdAt    context=${ngsild_test_suite_context}

    Purge Was Refused As Too Wide    ${response}
    Entity Still Exists

060_06_04 Purge Entities With A Query Over Only System Attributes
    [Documentation]    5.6.21.4 c) requires the query to include at least one non-system
    ...    Attribute; a query over modifiedAt alone does not qualify
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities
    ...    q=modifiedAt>"2020-01-01T00:00:00Z"
    ...    context=${ngsild_test_suite_context}

    Purge Was Refused As Too Wide    ${response}
    Entity Still Exists

060_06_05 Purge Entities With Local Scope Only Is Accepted
    [Documentation]    5.6.21.4 e): local scope alone IS sufficient — "If the execution of
    ...    the operation is limited to the local scope, no further restrictions have to be
    ...    provided" (5.6.21.3). Guards against over-tightening the check.
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities    local=true    context=${ngsild_test_suite_context}

    Should Not Be Equal As Integers
    ...    ${response.status_code}
    ...    400
    ...    local=true alone qualifies under 5.6.21.4 e)


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Purge Was Refused As Too Wide
    [Arguments]    ${response}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Entity Still Exists
    [Documentation]    The guard must run before anything is deleted
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
