*** Settings ***
Documentation       Check that one can query several entities based on scopes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query several entities based on scopes


*** Variables ***
${entity_one_scope}=        building-minimal-with-one-scope.json
${entity_many_scopes}=      building-minimal-with-many-scopes.json
${entity_type}=             https://ngsi-ld-test-suite/context#Building


*** Test Cases ***    SCOPEQ    EXPECTED_COUNT
019_01_06 QueryWithFullScope
    [Tags]    e-query    4_19    since_v1.5.1
    /Madrid/Gardens/ParqueNorte    ${2}
019_01_06 QueryWithPlusMatching
    [Tags]    e-query    4_19    since_v1.5.1
    /Madrid/+/ParqueNorte    ${2}
019_01_06 QueryWithHashMatching
    [Tags]    e-query    4_19    since_v1.5.1
    /CompanyA/#    ${1}
019_01_06 QueryNonEmptyScope
    [Tags]    e-query    4_19    since_v1.5.1
    /#    ${2}
019_01_06 QueryWithAndScope
    [Tags]    e-query    4_19    since_v1.5.1
    /Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC    ${1}
019_01_06 QueryWithOrScope
    [Tags]    e-query    4_19    since_v1.5.1
    /Madrid/Gardens/ParqueNorte,/CompanyA/OrganizationB/UnitC    ${2}


*** Keywords ***
Query several entities based on scopes
    [Documentation]    Check that one can query several entities based on scopes
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

Delete Entities
    Delete Entity    ${entity_one_scope_id}
    Delete Entity    ${entity_many_scopes_id}
