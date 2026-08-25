*** Settings ***
Documentation       Check that the Scope Query Language disjunction spellings and
...                 grouping parentheses are supported (CIM 009 clause 4.19).
...
...                 Normative sources: "Scopes are specified as a disjunction of elements,
...                 where each element can either directly be a Scope or a conjunction of
...                 multiple Scopes ... For logical and grouping parenthesis are needed";
...                 ABNF orOp = %x7C / %x2C (both "|" and ","), andOp = %x3B,
...                 OrScopeQ = %x28 ScopeQ *(andOp ScopeQ) %x29; EXAMPLE 4
...                 "(/Madrid/Districts;/CompanyA)" and EXAMPLE 5
...                 "(/Madrid/Districts;/CompanyA)|/CompanyB".
...
...                 Antares extension TP — 019_01_06 covers the bare "," and ";" spellings
...                 only; neither the "|" orOp nor the grouping parentheses of EXAMPLE 4/5
...                 are exercised anywhere in the suite.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Query Entities By Scope Expecting Count


*** Variables ***
${entity_one_scope}=        building-minimal-with-one-scope.json
${entity_many_scopes}=      building-minimal-with-many-scopes.json
${entity_type}=             https://ngsi-ld-test-suite/context#Building


*** Test Cases ***    SCOPEQ    EXPECTED_COUNT
019_34_01 Parenthesized Conjunction Selects Only The Entity Holding Both Scopes
    [Documentation]    4.19 EXAMPLE 4: "(/Madrid/Districts;/CompanyA)" — the parentheses
    ...    group a conjunction, so only an Entity carrying both Scopes is selected
    [Tags]    e-query    4_19    since_v1.9.1
    (/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)    ${1}

019_34_02 Pipe Is The Or Operator Over A Grouped Conjunction
    [Documentation]    4.19 EXAMPLE 5: "(/Madrid/Districts;/CompanyA)|/CompanyB" — the
    ...    conjunction and the trailing Scope are alternatives, so both Entities match
    [Tags]    e-query    4_19    since_v1.9.1
    (/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)|/Madrid/Gardens/ParqueNorte    ${2}

019_34_03 Pipe Is Equivalent To The Comma Between Plain Scopes
    [Documentation]    4.19 ABNF orOp = %x7C / %x2C: "a comma can be used as an alternative
    ...    representation of the or operator", so "|" selects the same Entities
    [Tags]    e-query    4_19    since_v1.9.1
    /CompanyA/OrganizationB/UnitC|/Nowhere/AtAll    ${1}

019_34_04 A Conjunction Fails Unless Every Scope Matches
    [Documentation]    4.19 andOp: a grouped conjunction requires every Scope, so one
    ...    unmatched Scope selects no Entity at all — the grouping parentheses must not
    ...    be read as part of a scope level either
    [Tags]    e-query    4_19    since_v1.9.1
    (/Madrid/Gardens/ParqueNorte;/Nowhere/AtAll)    ${0}


*** Keywords ***
Query Entities By Scope Expecting Count
    [Documentation]    Query Entities with the given scopeQ and check the number of
    ...    Entities in the response body
    [Arguments]    ${scopeq}    ${expected_count}
    ${response}=    Query Entities
    ...    scopeq=${scopeq}
    ...    entity_types=${entity_type}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Number Of Entities    ${entity_type}    ${expected_count}    ${response.json()}

Setup Initial Entities
    ${entity_one_scope_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_one_scope_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${entity_one_scope}
    ...    ${entity_one_scope_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response1.status_code}
    ${entity_many_scopes_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_many_scopes_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${entity_many_scopes}
    ...    ${entity_many_scopes_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Initial Entities
    Delete Entity    ${entity_one_scope_id}
    Delete Entity    ${entity_many_scopes_id}
