*** Settings ***
Documentation       Check that one can create an entity with one or more scopes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entity
Test Template       Create Entity Scenarios


*** Test Cases ***    FILENAME    CONTENT_TYPE
001_11_01 EntityWithOneScope
    [Tags]    e-create    4_18    since_v1.5.1
    building-minimal-with-one-scope.json    application/json
001_11_02 EntityWithManyScopes
    [Tags]    e-create    4_18    since_v1.5.1
    building-minimal-with-many-scopes.json    application/json


*** Keywords ***
Create Entity Scenarios
    [Documentation]    Check that one can create an entity with one or more scopes
    [Arguments]    ${filename}    ${content_type}
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}

    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${content_type}

    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${entity_id}    ${response.headers}
    ${created_entity}=    Load Test Sample    entities/${filename}    ${entity_id}
    ${response1}=    Retrieve Entity    ${entity_id}    ${content_type}
    Check Created Resource Set To    ${created_entity}    ${response1.json()}

Delete Initial Entity
    Delete Entity    ${entity_id}
